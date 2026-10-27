//! 诊断探针：独占下 `GetCurrentPadding` 与 `IAudioClock::GetPosition` 哪个可信，
//! 以及"按 in-flight 记账"的写入策略在事件/轮询两种时钟下是否都能喂饱设备。
//!
//! 背景：引擎在独占下按 `buffer - padding` 算可写空间，实测 1.5 秒只写了
//! 10688 帧（应 ≈72000），说明 padding 在独占下**根本不可信**。改用
//! `IAudioClock` 记账后轮询正常、事件仍饿死——这个探针就是用来把两种时钟
//! 分开看清楚。
//!
//! 每行：`pad` = GetCurrentPadding；`clk` = GetPosition 换算成帧；
//! `writ` = 累计写入；`fly` = writ - clk（在途帧，健康值 0..buffer 小幅波动）。
//!
//! 运行：cargo run -p aiya-audio --example clock_probe
//!      AIYA_MODE=events / AIYA_MODE=polling 可只跑一种

use std::time::{Duration, Instant};

use wasapi::{DeviceEnumerator, Direction, SampleType, StreamMode, WaveFormat};

const RATE: usize = 48_000;
const CH: usize = 2;
const RUN_MS: u64 = 2000;
fn buffer_ms() -> i64 {
    std::env::var("AIYA_BUFFER_MS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(200)
}

fn new_client() -> Result<wasapi::AudioClient, String> {
    let en = DeviceEnumerator::new().map_err(|e| e.to_string())?;
    let dev = en
        .get_default_device(&Direction::Render)
        .map_err(|e| e.to_string())?;
    dev.get_iaudioclient().map_err(|e| e.to_string())
}

/// 一次完整测量：返回 (写入帧数, 时钟增量帧, padding 取值种类数)。
fn run(events: bool) -> (u64, u64, usize) {
    let mut client = new_client().expect("client");
    let want = WaveFormat::new(32, 24, &SampleType::Int, RATE, CH, None);
    let negotiated = client
        .is_supported_exclusive_with_quirks(&want)
        .expect("设备不接受 i24 独占");
    let mode = if events {
        StreamMode::EventsExclusive { period_hns: buffer_ms() * 10_000 }
    } else {
        StreamMode::PollingExclusive {
            buffer_duration_hns: buffer_ms() * 10_000,
            period_hns: 10 * 10_000,
        }
    };
    client
        .initialize_client(&negotiated, &Direction::Render, &mode)
        .expect("initialize exclusive");

    let buffer_frames = client.get_buffer_size().expect("buffer size");
    let clock = client.get_audioclock().expect("audioclcok");
    let freq = clock.get_frequency().expect("clock freq");
    let render = client.get_audiorenderclient().expect("render");
    let event = if events {
        Some(client.set_get_eventhandle().expect("event"))
    } else {
        None
    };

    let block = CH * 4; // i24-in-32
    let buf = vec![0u8; buffer_frames as usize * block];
    let mut written: u64 = 0;
    let base = clock.get_position().map(|(p, _)| p).unwrap_or(0);

    client.start_stream().expect("start");
    let t0 = Instant::now();
    println!(
        "\n===== {}  buffer={buffer_frames} frames ({:.0}ms)  freq={freq} =====",
        if events {
            "独占 + 事件（周期=缓冲=200ms）"
        } else {
            "独占 + 轮询（缓冲 200ms / 周期 10ms）"
        },
        buffer_frames as f64 * 1000.0 / RATE as f64
    );
    println!("{:>8} {:>8} {:>8} {:>8} {:>8}", "t(ms)", "pad", "clk", "writ", "fly");

    let mut next_report = Duration::ZERO;
    let mut pads = std::collections::BTreeSet::new();
    let mut last_clk = 0u64;
    while t0.elapsed() < Duration::from_millis(RUN_MS) {
        if let Some(ev) = event.as_ref() {
            let _ = ev.wait_for_event(500);
        }
        let pad = client.get_current_padding().unwrap_or(0);
        pads.insert(pad);
        // 记账：可写 = buffer - (已写 - 已播)
        let (pos, _) = clock.get_position().unwrap_or((base, 0));
        let played = (pos.saturating_sub(base)).saturating_mul(RATE as u64) / freq.max(1);
        let in_flight = written.saturating_sub(played);
        let avail = (buffer_frames as u64).saturating_sub(in_flight) as usize;
        if avail > 0 && render.write_to_device(avail, &buf[..avail * block], None).is_ok() {
            written += avail as u64;
        }
        if event.is_none() {
            std::thread::sleep(Duration::from_millis(5));
        }
        if t0.elapsed() >= next_report {
            let (pos, _) = clock.get_position().unwrap_or((base, 0));
            last_clk = (pos.saturating_sub(base)).saturating_mul(RATE as u64) / freq.max(1);
            println!(
                "{:>8} {:>8} {:>8} {:>8} {:>8}",
                t0.elapsed().as_millis(),
                pad,
                last_clk,
                written,
                written.saturating_sub(last_clk)
            );
            next_report += Duration::from_millis(250);
        }
    }
    let _ = client.stop_stream();
    if let Some(ev) = event {
        drop(ev);
    }
    (written, last_clk, pads.len())
}

fn main() {
    { let _ = wasapi::initialize_mta(); }
    let only = std::env::var("AIYA_MODE").unwrap_or_default();
    if only != "polling" {
        let (w, c, pads) = run(true);
        println!("事件：写入 {w} 帧 / {RUN_MS}ms，时钟 {c} 帧，padding 出现 {pads} 种取值");
    }
    if only != "events" {
        let (w, c, pads) = run(false);
        println!("轮询：写入 {w} 帧 / {RUN_MS}ms，时钟 {c} 帧，padding 出现 {pads} 种取值");
    }
    { wasapi::deinitialize(); }
}
