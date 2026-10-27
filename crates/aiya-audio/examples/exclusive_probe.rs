//! 独占格式探测：枚举本机默认输出设备**实际接受**哪些独占格式。
//!
//! 用途（两个）：
//! 1. 设计依据——决定 `open_exclusive` 该尝试哪些 (采样率, 位深) 组合，而不是盲猜 f32；
//! 2. 排障工具——用户报"独占没生效"时，让他跑这个看设备底线。
//!
//! 只查询不建流，不出声、不占设备。用法：
//! `cargo run -p aiya-audio --example exclusive_probe`

use wasapi::{DeviceEnumerator, Direction, SampleType, ShareMode, WaveFormat};

fn main() {
    let _ = wasapi::initialize_mta(); // 探针跑在主线程，COM 要自己起
    let en = DeviceEnumerator::new().expect("DeviceEnumerator");
    let dev = en
        .get_default_device(&Direction::Render)
        .expect("默认输出设备");
    println!("默认输出设备：{}", dev.get_friendlyname().unwrap_or_default());

    let client = dev.get_iaudioclient().expect("IAudioClient");
    if let Ok(mix) = client.get_mixformat() {
        println!(
            "共享混音格式：{} Hz / {} ch / {} bit / {:?}",
            mix.get_samplespersec(),
            mix.get_nchannels(),
            mix.get_bitspersample(),
            mix.get_subformat().ok()
        );
    }
    let (def_period, min_period) = client.get_device_period().unwrap_or((0, 0));
    println!("设备周期：默认 {def_period} hns / 最小 {min_period} hns\n");

    // 位深候选：f32 32/32、i32 32/32、i24 装在 32 位里、i16
    let depths: [(&str, u16, u16, SampleType); 4] = [
        ("f32", 32, 32, SampleType::Float),
        ("i32", 32, 32, SampleType::Int),
        ("i24-in-32", 32, 24, SampleType::Int),
        ("i16", 16, 16, SampleType::Int),
    ];
    let rates = [44_100u32, 48_000, 88_200, 96_000, 192_000];
    let channels = [2usize, 1];

    println!("{:<12}{:<10}{:<6}独占", "格式", "采样率", "声道");
    println!("{}", "-".repeat(40));
    let mut any = false;
    for (name, bits, valid, st) in depths {
        for rate in rates {
            for ch in channels {
                let fmt = WaveFormat::new(bits as usize, valid as usize, &st, rate as usize, ch, None);
                // is_supported：Ok(None) = 精确支持；Ok(Some(f)) = 只支持最接近的 f（需改格式）
                let mark = match client.is_supported(&fmt, &ShareMode::Exclusive) {
                    Ok(None) => "✓ 精确",
                    Ok(Some(near)) => {
                        if near.get_samplespersec() == rate
                            && near.get_bitspersample() == bits
                            && near.get_nchannels() as usize == ch
                        {
                            "✓ 精确"
                        } else {
                            "~ 近似"
                        }
                    }
                    Err(_) => {
                        if client.is_supported_exclusive_with_quirks(&fmt).is_ok() {
                            "✓ 靠 quirk"
                        } else {
                            "✗"
                        }
                    }
                };
                if mark.starts_with('✓') {
                    any = true;
                }
                println!("{name:<12}{rate:<10}{ch:<6}{mark}");
            }
        }
    }
    if !any {
        println!("\n结论：设备拒绝全部候选 → 独占模式只能回落共享（功能仍可用，音质仍由共享混音决定）");
    }
}
