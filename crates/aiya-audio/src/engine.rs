//! WASAPI 共享模式输出（事件驱动）。
//!
//! 关键决策（docs/04 §2）：`EventsShared { autoconvert: true }` ——
//! 引擎自己完成 源格式→mix format 的转换与 SRC（SRC_DEFAULT_QUALITY），
//! 我们只交源采样率的 f32 交错样本，v1 不自建重采样器。
//! 独占模式（bit-perfect + 链内 DSP）走同一 render 循环的 EventsExclusive 分支，P2 后补。

use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicU64};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::Arc;
use std::time::Duration;

use wasapi::DeviceEnumerator;
use wasapi::{Direction, SampleType, StreamMode, WaveFormat};

use crate::decoder::Decoder;
use crate::AudioError;

/// 引擎线程收到的命令。
#[derive(Debug, Clone)]
pub enum EngineCommand {
    /// 装载并播放
    Load(String),
    Pause,
    Resume,
    /// 跳到指定毫秒（若未装载则忽略）
    Seek(u64),
    /// 0.0–1.0
    SetVolume(f32),
    Stop,
    /// 退出线程
    Shutdown,
}

/// 引擎线程发出的回报。
#[derive(Debug, Clone)]
pub enum EngineEvent {
    /// 装载成功（时长毫秒）
    Loaded { duration_ms: u64 },
    /// 播放位置推进（渲染循环节拍发出，~每 10ms）
    Position { position_ms: u64 },
    /// 曲目自然播完
    Eof,
    /// 状态变化（供 PlayerActor 对齐）
    StateChanged { playing: bool },
    Error { message: String },
}

/// 引擎句柄：命令进 / 事件出。Drop 不强制停线程（Shutdown 命令负责）。
pub struct Engine {
    tx: Sender<EngineCommand>,
    rx: std::sync::mpsc::Receiver<EngineEvent>,
    worker: Option<std::thread::JoinHandle<()>>,
    /// 装载后由 Load 事件填充
    pub duration_ms: Arc<AtomicU64>,
    pub playing: Arc<AtomicBool>,
}

impl Engine {
    pub fn spawn() -> Result<Self, AudioError> {
        let (cmd_tx, cmd_rx) = mpsc::channel::<EngineCommand>();
        let (ev_tx, ev_rx) = mpsc::channel::<EngineEvent>();
        let worker = std::thread::Builder::new()
            .name("aiya-audio-engine".into())
            .spawn(move || render_loop(cmd_rx, ev_tx))
            .map_err(|e| AudioError::Custom(e.to_string()))?;
        Ok(Self {
            tx: cmd_tx,
            rx: ev_rx,
            worker: Some(worker),
            duration_ms: Arc::new(AtomicU64::new(0)),
            playing: Arc::new(AtomicBool::new(false)),
        })
    }

    pub fn send(&self, cmd: EngineCommand) {
        let _ = self.tx.send(cmd);
    }

    /// 非阻塞取事件（PlayerActor 轮询）。
    pub fn try_recv(&self) -> Option<EngineEvent> {
        self.rx.try_recv().ok()
    }

    pub fn shutdown(&mut self) {
        let _ = self.tx.send(EngineCommand::Shutdown);
        if let Some(w) = self.worker.take() {
            let _ = w.join();
        }
    }
}

impl Drop for Engine {
    fn drop(&mut self) {
        self.shutdown();
    }
}

/// 渲染循环：COM init → 设备 → mix format → 事件驱动写流。
fn render_loop(cmd_rx: Receiver<EngineCommand>, ev_tx: Sender<EngineEvent>) {
    let send = |e: EngineEvent| {
        let _ = ev_tx.send(e);
    };

    { let _ = wasapi::initialize_mta(); }

    'outer: loop {
        // 命令循环：等 Load 或 Shutdown
        let path = loop {
            match cmd_rx.recv() {
                Ok(EngineCommand::Load(p)) => break p,
                Ok(EngineCommand::Shutdown) | Err(_) => break 'outer,
                Ok(other) => {
                    // 未装载时只响应 Stop 类命令的回报
                    handle_preload(&other, &send);
                }
            }
        };

        match play_file(&cmd_rx, &send, &path) {
            Ok(()) => {}
            Err(e) => {
                tracing::error!(%path, error = %e, "engine stream failed");
                send(EngineEvent::Error { message: e.to_string() });
                // 回到命令循环等下一条 Load（设备恢复）
            }
        }
    }

    { let _ = wasapi::deinitialize(); }
}

fn handle_preload(cmd: &EngineCommand, send: &impl Fn(EngineEvent)) {
    match cmd {
        EngineCommand::Stop => send(EngineEvent::StateChanged { playing: false }),
        _ => {}
    }
}

/// 打开文件并进入渲染循环直到 Stop/Shutdown/新 Load/EOF。
fn play_file(
    cmd_rx: &Receiver<EngineCommand>,
    send: &impl Fn(EngineEvent),
    path: &str,
) -> Result<(), AudioError> {
    let mut decoder = Decoder::open(Path::new(path))?;
    send(EngineEvent::Loaded { duration_ms: decoder.duration_ms });

    let enumerator = DeviceEnumerator::new().map_err(wasapi_err)?;
    let device = enumerator.get_default_device(&Direction::Render).map_err(wasapi_err)?;
    let mut client = device.get_iaudioclient().map_err(wasapi_err)?;

    // 源格式：f32 交错，让引擎做转换（autoconvert: true）
    let src_fmt = WaveFormat::new(
        32,
        32,
        &SampleType::Float,
        decoder.sample_rate as usize,
        decoder.channels,
        None,
    );
    client
        .initialize_client(
            &src_fmt,
            &Direction::Render,
            &StreamMode::EventsShared { autoconvert: true, buffer_duration_hns: 2_000_000 }, // 200ms
        )
        .map_err(wasapi_err)?;
    let event = client.set_get_eventhandle().map_err(wasapi_err)?;
    let render = client.get_audiorenderclient().map_err(wasapi_err)?;
    let buffer_frames = client.get_buffer_size().map_err(wasapi_err)?;

    let bytes_per_sample = 4usize;
    let frame_bytes = decoder.channels * bytes_per_sample;
    // 一帧的原始字节缓冲（f32 交错）
    let mut sample_bytes = vec![0u8; buffer_frames as usize * frame_bytes];
    // 跨包剩余：上一包没写完的 f32 样本（设备空间 < 一包时暂存，下轮优先写）
    let mut leftover: Vec<f32> = Vec::new();

    let mut volume: f32 = 1.0;
    let mut paused = false;
    let mut user_stopped = false;
    client.start_stream().map_err(wasapi_err)?;
    send(EngineEvent::StateChanged { playing: true });

    let mut result: Result<(), AudioError> = Ok(());

    loop {
        // 等引擎缓冲区消费事件（暂停时也等，只是不写数据）
        event.wait_for_event(2000).map_err(|e| AudioError::Custom(e.to_string()))?;

        // 非阻塞处理命令
        loop {
            match cmd_rx.try_recv() {
                Ok(EngineCommand::Pause) if !paused => {
                    paused = true;
                    let _ = client.stop_stream().map_err(wasapi_err);
                    send(EngineEvent::StateChanged { playing: false });
                }
                Ok(EngineCommand::Resume) if paused => {
                    paused = false;
                    client.start_stream().map_err(wasapi_err)?;
                    send(EngineEvent::StateChanged { playing: true });
                }
                Ok(EngineCommand::Seek(ms)) => {
                    // 停流 → seek → reset（清空设备缓冲）→ 重启
                    let _ = client.stop_stream().map_err(wasapi_err);
                    if let Err(e) = decoder.seek_ms(ms) {
                        result = Err(e);
                        break;
                    }
                    let _ = client.reset_stream().map_err(wasapi_err);
                    client.start_stream().map_err(wasapi_err)?;
                    send(EngineEvent::Position { position_ms: decoder.position_ms() });
                }
                Ok(EngineCommand::SetVolume(v)) => volume = v.clamp(0.0, 1.0),
                Ok(EngineCommand::Stop) => {
                    user_stopped = true;
                    break;
                }
                Ok(EngineCommand::Load(_)) | Ok(EngineCommand::Shutdown) => {
                    // 交给外层：先把当前流收尾，外层循环重新进入
                    return finalize(client, result);
                }
                Ok(_) => {}
                Err(_) => break,
            }
        }
        if user_stopped {
            send(EngineEvent::StateChanged { playing: false });
            send(EngineEvent::Eof); // Stop 视为曲目结束（PlayerActor 决定下一动作）
            let _ = client.stop_stream().map_err(wasapi_err);
            return Ok(());
        }
        if result.is_err() {
            return finalize(client, result);
        }
        if paused {
            continue;
        }

        // 写满可用空间
        let padding = client.get_current_padding().map_err(wasapi_err)?;
        let available = buffer_frames - padding;
        if available == 0 {
            send(EngineEvent::Position { position_ms: decoder.position_ms() });
            continue;
        }
        // 每轮渲染心跳都上报位置（泵侧再做 500ms 节流；写满瞬间 available 不会为 0）
        send(EngineEvent::Position { position_ms: decoder.position_ms() });

        let mut frames_to_write = available as usize;
        let mut eof_now = false;
        let channels = decoder.channels.max(1);

        'fill: while frames_to_write > 0 {
            // 1) 先消耗 leftover（上轮没写完的样本）
            let (take_samples, from_leftover) = if !leftover.is_empty() {
                let frames = (leftover.len() / channels).min(frames_to_write);
                (frames * channels, true)
            } else {
                (0, false)
            };
            if take_samples > 0 {
                let frame_bytes_all = channels * 4;
                let out = &mut sample_bytes[..take_samples / channels * frame_bytes_all];
                let mut bi = 0usize;
                for ch in 0..take_samples {
                    let s = leftover[ch] * volume;
                    out[bi..bi + 4].copy_from_slice(&s.to_le_bytes());
                    bi += 4;
                }
                let frames = take_samples / channels;
                render
                    .write_to_device(frames, &sample_bytes[..frames * frame_bytes_all], None)
                    .map_err(wasapi_err)?;
                leftover.drain(..take_samples);
                frames_to_write -= frames;
                if frames_to_write == 0 {
                    break 'fill;
                }
            }
            let _ = from_leftover;

            // 2) 解码新包
            match decoder.next_packet() {
                Ok(Some(samples)) => {
                    let packet_frames = samples.len() / channels;
                    let take_frames = packet_frames.min(frames_to_write);
                    let mut bi = 0usize;
                    let frame_bytes_all = channels * 4;
                    let out = &mut sample_bytes[..take_frames * frame_bytes_all];
                    for f in 0..take_frames {
                        for ch in 0..channels {
                            let s = samples[f * channels + ch] * volume;
                            out[bi..bi + 4].copy_from_slice(&s.to_le_bytes());
                            bi += 4;
                        }
                    }
                    render
                        .write_to_device(take_frames, &sample_bytes[..take_frames * frame_bytes_all], None)
                        .map_err(wasapi_err)?;
                    frames_to_write -= take_frames;
                    // 没写完的部分进 leftover（下轮优先写，不丢样本）
                    leftover.clear();
                    leftover.extend_from_slice(&samples[take_frames * channels..]);
                    if frames_to_write == 0 {
                        break 'fill;
                    }
                }
                Ok(None) => {
                    eof_now = true;
                    break 'fill;
                }
                Err(e) => {
                    result = Err(e);
                    break 'fill;
                }
            }
        }

        if eof_now {
            // 等缓冲播完再报 EOF：padding 归零或超时
            let deadline = std::time::Instant::now() + Duration::from_millis(500);
            loop {
                let pad = client.get_current_padding().map_err(wasapi_err)?;
                if pad == 0 || std::time::Instant::now() > deadline {
                    break;
                }
                std::thread::sleep(Duration::from_millis(10));
            }
            let _ = client.stop_stream().map_err(wasapi_err);
            send(EngineEvent::Position { position_ms: decoder.duration_ms });
            send(EngineEvent::StateChanged { playing: false });
            send(EngineEvent::Eof);
            // EOF 后回命令循环等下一条 Load（外层 'outer）
            return Ok(());
        }
    }
}

/// 收尾：停流并返回原 result（用于把内部错误传出）。
fn finalize(
    client: wasapi::AudioClient,
    result: Result<(), AudioError>,
) -> Result<(), AudioError> {
    let _ = client.stop_stream();
    result
}

fn wasapi_err(e: wasapi::WasapiError) -> AudioError {
    AudioError::Wasapi(e.to_string())
}
