//! 输出组合矩阵实测：共享/独占 × 事件驱动/轮询 × 三种写入策略。
//!
//! ⚠️ 2026-10-25 更正：本工具的 `lag = 已写 - 墙钟*采样率` 判据**不能识别
//! "一帧都没写"**——写出 0 帧时 lag 同样恒定，于是被读成"正常"。据此得出的
//! "独占轮询 accounted 正常"是错的：实际只写了首缓冲就卡死。
//! 现在判"声音是否在走"请用 `examples/clock_probe.rs`（打印 padding/时钟/写入）
//! 或 `cargo run --example mode_smoke`（对写入速率做硬断言）。本工具只保留作
//! 缓冲区/事件间隔的粗测。
//!
//! 目的：给"要不要把四种组合都做成设置"提供数据。每种组合真实开流、连续写静音，
//! 记录缓冲区、事件间隔、以及**写入是否跟得上设备消费**。
//!
//! 判据 `lag = 已写帧数 - 墙钟*采样率`：
//! * 健康的流 ≈ 0..buffer_frames（领先设备一个缓冲深度），且不随时间漂移；
//! * 持续变负 = 欠载（设备在插静音），即"放不出声"。
//!
//! 两种策略：
//! * `padding`：引擎当前做法，available = buffer - GetCurrentPadding()；
//! * `accounted`：自己记账，可写 = 已消费 + buffer - 已写，完全不问 padding。
//!
//! 运行：npm run probe:matrix

use std::time::{Duration, Instant};

use wasapi::{DeviceEnumerator, Direction, SampleType, StreamMode, WaveFormat};

static RATE_V: std::sync::OnceLock<usize> = std::sync::OnceLock::new();
fn rate() -> usize {
    *RATE_V.get_or_init(|| {
        std::env::var("AIYA_RATE").ok().and_then(|s| s.parse().ok()).unwrap_or(44_100)
    })
}
const CH: usize = 2;
const RUN_MS: u64 = 2000;

#[derive(Clone, Copy, PartialEq)]
enum Share {
    Shared,
    Exclusive,
}

#[derive(Clone, Copy, PartialEq)]
enum Clock {
    Events,
    Polling,
}

#[derive(Clone, Copy, PartialEq)]
enum Strategy {
    Padding,
    Accounted,
    /// 每个事件写满一个缓冲（事件驱动独占的标准做法）
    Exact,
}

struct Opened {
    client: wasapi::AudioClient,
    label: String,
    block_align: usize,
    buffer_frames: u32,
}

fn new_client() -> Result<wasapi::AudioClient, String> {
    let en = DeviceEnumerator::new().map_err(|e| e.to_string())?;
    let dev = en
        .get_default_device(&Direction::Render)
        .map_err(|e| e.to_string())?;
    dev.get_iaudioclient().map_err(|e| e.to_string())
}

fn open(share: Share, clock: Clock, period_hns: Option<i64>, buf_hns: i64) -> Result<Opened, String> {
    if share == Share::Shared {
        let mut client = new_client()?;
        let fmt = WaveFormat::new(32, 32, &SampleType::Float, rate(), CH, None);
        let mode = match clock {
            Clock::Events => StreamMode::EventsShared {
                autoconvert: true,
                buffer_duration_hns: buf_hns,
            },
            Clock::Polling => StreamMode::PollingShared {
                autoconvert: true,
                buffer_duration_hns: buf_hns,
            },
        };
        client
            .initialize_client(&fmt, &Direction::Render, &mode)
            .map_err(|e| format!("{e}"))?;
        let buffer_frames = client.get_buffer_size().map_err(|e| e.to_string())?;
        return Ok(Opened {
            client,
            label: "f32".into(),
            block_align: CH * 4,
            buffer_frames,
        });
    }

    // 独占：与引擎同一条降级链（f32 → i24-in-32 → i16），只接受源采样率/声道
    let candidates: [(usize, usize, SampleType); 3] = [
        (32, 32, SampleType::Float),
        (32, 24, SampleType::Int),
        (16, 16, SampleType::Int),
    ];
    let mut errors = Vec::new();
    for (bits, valid, sub) in candidates {
        let mut client = new_client()?;
        let want = WaveFormat::new(bits, valid, &sub, rate(), CH, None);
        let negotiated = match client.is_supported_exclusive_with_quirks(&want) {
            Ok(f) => f,
            Err(e) => {
                errors.push(format!("{bits}bit 探测: {e}"));
                continue;
            }
        };
        let label = match (negotiated.get_bitspersample(), negotiated.get_subformat().ok()) {
            (32, Some(SampleType::Float)) => "f32",
            (32, Some(SampleType::Int)) => "i24-in-32",
            (16, _) => "i16",
            (b, _) => {
                errors.push(format!("{b}bit 不支持"));
                continue;
            }
        };
        let (default_period, _min) = client.get_device_period().map_err(|e| e.to_string())?;
        let period = period_hns.unwrap_or(default_period);
        let mode = match clock {
            Clock::Events => StreamMode::EventsExclusive { period_hns: period },
            Clock::Polling => StreamMode::PollingExclusive {
                period_hns: period,
                buffer_duration_hns: buf_hns,
            },
        };
        match client.initialize_client(&negotiated, &Direction::Render, &mode) {
            Ok(()) => {
                let buffer_frames = client.get_buffer_size().map_err(|e| e.to_string())?;
                return Ok(Opened {
                    client,
                    label: label.into(),
                    block_align: if bits == 16 { CH * 2 } else { CH * 4 },
                    buffer_frames,
                });
            }
            Err(e) => errors.push(format!("{label} init: {e}")),
        }
    }
    Err(errors.join("; "))
}

struct Stats {
    written: u64,
    elapsed: f64,
    /// 已写 - 墙钟*rate()：健康值 ≈ 0..buffer，持续下降 = 欠载
    lag_start: i64,
    lag_end: i64,
    padding_min: u32,
    padding_max: u32,
    intervals: Vec<u64>,
}

fn run(op: &Opened, clock: Clock, strategy: Strategy, period: Duration) -> Stats {
    let client = &op.client;
    let buf = op.buffer_frames;
    let ba = op.block_align;
    let zeros = vec![0u8; buf as usize * ba];
    let event = if clock == Clock::Events {
        client.set_get_eventhandle().ok()
    } else {
        None
    };
    let render = client.get_audiorenderclient().expect("render client");

    let mut written: u64 = 0;
    let mut padding_min = u32::MAX;
    let mut padding_max = 0u32;
    let mut intervals = Vec::new();

    let t0 = Instant::now();
    // 正确顺序：先填满缓冲再启动（wasapi-rs 文档明确要求；当前引擎是空缓冲直接 start）
    let prefill = match strategy {
        Strategy::Padding | Strategy::Exact => 0, // 复现引擎行为：不预填
        Strategy::Accounted => buf as usize,
    };
    if prefill > 0 {
        let _ = render.write_to_device(prefill, &zeros[..prefill * ba], None);
        written += prefill as u64;
    }
    client.start_stream().expect("start");
    let mut last_event = t0;
    let mut lag_start = 0i64;

    while t0.elapsed() < Duration::from_millis(RUN_MS) {
        match &event {
            Some(h) => {
                let _ = h.wait_for_event(2000);
                let now = Instant::now();
                intervals.push((now - last_event).as_micros() as u64);
                last_event = now;
            }
            None => std::thread::sleep(period / 2),
        }

        let padding = client.get_current_padding().unwrap_or(0);
        padding_min = padding_min.min(padding);
        padding_max = padding_max.max(padding);

        let writable = match strategy {
            Strategy::Padding => buf.saturating_sub(padding) as usize,
            Strategy::Exact => buf as usize,
            Strategy::Accounted => {
                // 设备按 rate() 恒速消费；可写 = 已消费 + 缓冲深度 - 已写
                let consumed = (t0.elapsed().as_secs_f64() * rate() as f64) as i64;
                let due = consumed + i64::from(buf) - written as i64;
                due.clamp(0, i64::from(buf)) as usize
            }
        };
        if writable > 0 {
            if render
                .write_to_device(writable, &zeros[..writable * ba], None)
                .is_ok()
            {
                written += writable as u64;
            }
        }

        let elapsed = t0.elapsed().as_secs_f64();
        let lag = written as i64 - (elapsed * rate() as f64) as i64;
        if lag_start == 0 {
            lag_start = lag;
        }
    }

    let elapsed = t0.elapsed().as_secs_f64();
    let _ = client.stop_stream();
    Stats {
        written,
        elapsed,
        lag_start,
        lag_end: written as i64 - (elapsed * rate() as f64) as i64,
        padding_min: if padding_min == u32::MAX { 0 } else { padding_min },
        padding_max,
        intervals,
    }
}

fn pctl(v: &mut [u64], p: f64) -> u64 {
    if v.is_empty() {
        return 0;
    }
    v.sort_unstable();
    v[((v.len() as f64 - 1.0) * p).round() as usize]
}

fn main() {
    let arg = std::env::args().nth(1).unwrap_or_default();
    let only_exclusive = arg == "--exclusive";
    let hr = wasapi::initialize_mta();
    println!("COM MTA: {hr:?}");

    let default_period = {
        let en = DeviceEnumerator::new().expect("enumerator");
        let dev = en.get_default_device(&Direction::Render).expect("device");
        println!("默认输出设备：{}", dev.get_friendlyname().unwrap_or_default());
        let c = new_client().expect("client");
        if let Ok(mix) = c.get_mixformat() {
            println!(
                "设备 mix format：{} Hz / {} ch / {} bit",
                mix.get_samplespersec(),
                mix.get_nchannels(),
                mix.get_bitspersample()
            );
        }
        let (d, m) = c.get_device_period().unwrap_or((100_000, 30_000));
        println!(
            "设备周期：默认 {:.2} ms，最小 {:.2} ms",
            d as f64 / 10_000.0,
            m as f64 / 10_000.0
        );
        Duration::from_nanos(d as u64 * 100)
    };

    println!(
        "\n{} Hz / {} ch，每栏 {} ms，静音\n",
        rate(), CH, RUN_MS
    );
    println!(
        "{:<38} {:>10} {:>6} {:>9} {:>15} {:>7} {:>6}",
        "组合 / 策略", "格式", "buf", "pad范围", "lag 首→末", "事件p99", "结论"
    );
    println!("{}", "-".repeat(96));

    let mut cases: Vec<(&str, Share, Clock, Option<i64>, i64, Strategy)> = vec![];
    if !only_exclusive {
        cases.push(("shared+events / padding", Share::Shared, Clock::Events, None, 2_000_000, Strategy::Padding));
        cases.push(("shared+events / accounted", Share::Shared, Clock::Events, None, 2_000_000, Strategy::Accounted));
        cases.push(("shared+polling / accounted", Share::Shared, Clock::Polling, None, 2_000_000, Strategy::Accounted));
    }
    cases.push(("excl+events(默认周期) / padding", Share::Exclusive, Clock::Events, None, 0, Strategy::Padding));
    cases.push(("excl+events(20ms) / padding", Share::Exclusive, Clock::Events, Some(200_000), 0, Strategy::Padding));
    cases.push(("excl+events(20ms) / accounted", Share::Exclusive, Clock::Events, Some(200_000), 0, Strategy::Accounted));
    cases.push(("excl+events(20ms) / exact", Share::Exclusive, Clock::Events, Some(200_000), 0, Strategy::Exact));
    cases.push(("excl+polling(200ms) / accounted", Share::Exclusive, Clock::Polling, None, 2_000_000, Strategy::Accounted));
    cases.push(("excl+polling(200ms) / padding", Share::Exclusive, Clock::Polling, None, 2_000_000, Strategy::Padding));

    for (name, share, clock, period_override, buf_hns, strategy) in cases {
        match open(share, clock, period_override, buf_hns) {
            Ok(op) => {
                let period = period_override
                    .map(|p| Duration::from_nanos(p as u64 * 100))
                    .unwrap_or(default_period);
                let mut st = run(&op, clock, strategy, period);
                let p99 = if st.intervals.is_empty() {
                    "-".to_string()
                } else {
                    format!("{}ms", pctl(&mut st.intervals, 0.99) / 1000)
                };
                let verdict = if st.lag_end < -2 * i64::from(op.buffer_frames) {
                    "欠载"
                } else if st.lag_end > i64::from(op.buffer_frames) * 3 {
                    "超写"
                } else {
                    "正常"
                };
                println!(
                    "{:<38} {:>10} {:>6} {:>4}-{:<4} {:>7}→{:<8} {:>7} {:>6}",
                    name,
                    op.label,
                    op.buffer_frames,
                    st.padding_min,
                    st.padding_max,
                    st.lag_start,
                    st.lag_end,
                    p99,
                    verdict
                );
                let _ = (st.written, st.elapsed);
            }
            Err(e) => println!("{:<38} 打开失败：{e}", name),
        }
    }
    println!();
    let _ = wasapi::deinitialize();
}
