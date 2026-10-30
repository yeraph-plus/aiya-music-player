//! MPEG-TS 段剥离：188 字节包 → 目标 ES 的裸字节（docs/52 §3）。
//!
//! **这不是一个 demuxer**。HLS 规范要求 TS 里的 AAC 用 ADTS 封装，所以只要
//! 剥掉 TS 包层和 PES 头，剩下的字节就是现成 `probe(hint=aac)` 认得的东西——
//! 后面整条链（`DecodedStream` / engine / WASAPI / 频谱）一行都不用动。
//!
//! ```text
//! TS 段（188B 包） → PMT 找音频 ES_PID → 拼载荷 → 剥 PES 头 → ADTS / MPEG audio
//! ```
//!
//! 简化的前提（音频-only 直播段，实测 6 个真台全部符合）：
//! * 不处理视频（认出来也不解，跳过）；
//! * 不处理多节目（取第一个 `program_number != 0` 的 PMT）；
//! * 不处理 B 帧重排、不读时间戳（播放位置由 `HlsStream` 按已播帧数累加）；
//! * PAT/PMT 可能只出现在首段 → [`TsState`] 必须跨段存活（挂在 `HlsStream` 上）。
//!
//! 为什么用内容嗅探而不是扩展名：实测 Power POP 的段名带查询串
//! （`..._6570.ts?nimblesessionid=...`），`ends_with(".ts")` 不匹配 → 被当成
//! ADTS 段收下 → 到 probe 才炸，报的是"段解不开"而不是"格式不支持"。

use crate::AudioError;

/// 段里音频 ES 的封装种类。决定 probe 的 hint：
/// * [`TsAudio::Adts`]：`stream_type 0x0F`，AAC 直接 ADTS 封装（最常见）；
/// * [`TsAudio::MpegAudio`]：`stream_type 0x03/0x04`，MP1/MP2/MP3 裸帧。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TsAudio {
    Adts,
    MpegAudio,
}

impl TsAudio {
    /// probe 用的扩展名（Symphonia 的 `Hint`）。
    pub fn hint_ext(self) -> &'static str {
        match self {
            TsAudio::Adts => "aac",
            TsAudio::MpegAudio => "mp3",
        }
    }
}

/// 跨段存活的 PMT/ES 定位状态。
///
/// **为什么必须跨段**：有的分段器只在首段发 PAT/PMT，后续段只有 PES 载荷。
/// 每段都当"全新"解析的话，从第二段起就报"这段没有 PAT/PMT"。
#[derive(Debug, Default, Clone, Copy)]
pub struct TsState {
    /// PAT 里的 PMT PID
    pmt_pid: Option<u16>,
    /// PMT 里的音频 ES PID
    audio_pid: Option<u16>,
    /// 音频 ES 的封装种类（与 `audio_pid` 同生同灭）
    audio_kind: Option<TsAudio>,
}

impl TsState {
    pub fn new() -> Self {
        Self::default()
    }

    /// 已定位到的音频封装种类（跨段缓存；首段之前是 None）。
    pub fn audio_kind(&self) -> Option<TsAudio> {
        self.audio_kind
    }
}

/// TS 包长（DVB / HLS 用 188；192 是蓝光 M2TS，不在范围内）。
const PACKET_LEN: usize = 188;
/// 同步字节
const SYNC: u8 = 0x47;
/// PAT 固定 PID
const PID_PAT: u16 = 0x0000;

/// 内容嗅探：首字节同步字 + 第二个包也在位。
///
/// 不用 `len % 188 == 0` 单独判：截断的坏段会因此漏判，落在 ADTS 路径上报
/// "段解不开"。长度校验留给 [`strip`]，那里能报出具体差多少。
pub fn is_ts(bytes: &[u8]) -> bool {
    if bytes.len() < PACKET_LEN || bytes[0] != SYNC {
        return false;
    }
    // 只有一个包时无法交叉验证，只能信同步字（单包段在实际流里不存在）
    bytes.len() == PACKET_LEN || bytes.get(PACKET_LEN) == Some(&SYNC)
}

/// 剥一段 TS：返回（目标 ES 裸字节, 封装种类）。
///
/// `st` 跨段复用——本段没 PAT/PMT 时用上一段的定位结果。
pub fn strip(bytes: &[u8], st: &mut TsState) -> Result<(Vec<u8>, TsAudio), AudioError> {
    if bytes.is_empty() {
        return Err(AudioError::Decode("TS 段是空的".into()));
    }
    if !bytes.len().is_multiple_of(PACKET_LEN) {
        return Err(AudioError::Decode(format!(
            "TS 段长度 {} 不是 188 的整数倍（段被截断或不是 TS）",
            bytes.len()
        )));
    }

    let mut out: Vec<u8> = Vec::with_capacity(bytes.len() / 2);
    // 本段是否真的见过目标 PID（见函数尾的"没有音频数据"判定）
    let mut saw_audio_packet = false;
    let mut pes = PesAssembler::default();

    for (idx, packet) in bytes.as_chunks::<PACKET_LEN>().0.iter().enumerate() {
        if packet[0] != SYNC {
            return Err(AudioError::Decode(format!(
                "TS 第 {} 个包失去同步（首字节 0x{:02X} 不是 0x47）",
                idx + 1,
                packet[0]
            )));
        }
        // transport_error_indicator：对端已标记该包损坏，丢掉比猜好
        if packet[1] & 0x80 != 0 {
            continue;
        }
        let pid = (((packet[1] & 0x1F) as u16) << 8) | packet[2] as u16;
        let pusi = packet[1] & 0x40 != 0;
        let scrambling = (packet[3] >> 6) & 0b11;
        if scrambling != 0 {
            continue; // 加密节目（与 AES HLS 不同，这是 TS 层加扰，不在范围内）
        }
        let afc = (packet[3] >> 4) & 0b11;

        // 载荷起点：0b11 先跳 adaptation field（`data[4]` 是它的长度，不含自身）
        let payload_start = match afc {
            0b00 => continue, // 保留值，按规范丢弃
            0b10 => continue, // 只有 adaptation field，无载荷
            0b11 => 5 + packet[4] as usize,
            _ => 4, // 0b01：纯载荷
        };
        if payload_start >= PACKET_LEN {
            continue; // adaptation field 把整个包吃掉了
        }
        let payload = &packet[payload_start..];

        if pid == PID_PAT {
            parse_pat(payload, st);
        } else if Some(pid) == st.pmt_pid {
            // PMT 里可能是视频节目（没有音频 ES）——不当作错误，继续等
            parse_pmt(payload, st)?;
        } else if Some(pid) == st.audio_pid {
            saw_audio_packet = true;
            pes.feed(payload, pusi, &mut out);
        }
    }

    let kind = st.audio_kind.ok_or_else(|| {
        AudioError::Unsupported("这段 TS 里没有 PAT/PMT，定位不到音频流（HLS 段应带节目表）".into())
    })?;
    if out.is_empty() {
        // 有 PMT 但没收到载荷：段被截断，或音频 PID 在本段才出现
        let why = if saw_audio_packet {
            "音频包只有 PES 头，没有载荷"
        } else {
            "本段没有任何音频包"
        };
        return Err(AudioError::Decode(format!("TS 段无音频数据：{why}")));
    }
    Ok((out, kind))
}

/// PAT：找第一个 `program_number != 0` 的 PMT PID。
///
/// 跨包 PAT 不处理（`section_length` 大于单包载荷时放弃）——音频-only 直播的
/// PAT 只有一条节目，几十字节，现实中没见过跨包。真遇上就是"定位不到音频流"
/// 的明确报错，不会静默放错流。
fn parse_pat(payload: &[u8], st: &mut TsState) {
    let Some(sec) = section(payload, 0x00) else {
        return;
    };
    // 表头 8 字节：table_id(1) + section_length(2) + tsid(2) + version(1)
    //             + section_number(1) + last_section_number(1)
    for entry in sec.get(8..).unwrap_or_default().as_chunks::<4>().0.iter() {
        let program = u16::from_be_bytes([entry[0], entry[1]]);
        if program == 0 {
            continue; // 0 = network PID（NIT），不是节目
        }
        st.pmt_pid = Some((((entry[2] & 0x1F) as u16) << 8) | entry[3] as u16);
        return;
    }
}

/// PMT：挑第一个能解的音频 ES（`0x0F` ADTS / `0x03` `0x04` MPEG audio）。
fn parse_pmt(payload: &[u8], st: &mut TsState) -> Result<(), AudioError> {
    let Some(sec) = section(payload, 0x02) else {
        return Ok(());
    };
    if sec.len() < 12 {
        return Ok(());
    }
    // 表头 12 字节：table_id(1) + section_length(2) + program_number(2)
    //   + version(1) + section_number(1) + last_section_number(1) + PCR_PID(2)
    //    + program_info_length(2)
    // 长度字段都是 12 位：高字节低 4 位有效（高 4 位是保留位，按规范为 0b1111）
    let program_info_len = (((sec[10] & 0x0F) as usize) << 8) | sec[11] as usize;
    let end = sec.len();
    let mut i = 12 + program_info_len;
    // 只有 LATM 时给一句准确的话（否则落到"没有音频流"，用户以为文件坏了）
    let mut saw_latm = false;

    while i + 5 <= end {
        let stream_type = sec[i];
        let pid = (((sec[i + 1] & 0x1F) as u16) << 8) | sec[i + 2] as u16;
        let es_info_len = (((sec[i + 3] & 0x0F) as usize) << 8) | sec[i + 4] as usize;
        let kind = match stream_type {
            0x0F => Some(TsAudio::Adts),
            0x03 | 0x04 => Some(TsAudio::MpegAudio),
            0x11 => {
                saw_latm = true;
                None
            }
            _ => None, // 视频 / 字幕 / 私有数据：跳过
        };
        if let Some(kind) = kind {
            st.audio_pid = Some(pid);
            st.audio_kind = Some(kind);
            return Ok(());
        }
        i += 5 + es_info_len;
    }

    if saw_latm && st.audio_pid.is_none() {
        return Err(AudioError::Unsupported(
            "LATM/LOAS 封装的 AAC 不支持（本播放器只剥 ADTS）".into(),
        ));
    }
    Ok(())
}

/// 取 section 切片：先跳 `pointer_field`，再校 `table_id` 与 `section_length`
/// 的边界（避免越界读）。
fn section(payload: &[u8], want_table_id: u8) -> Option<&[u8]> {
    let ptr = *payload.first()? as usize;
    let sec = payload.get(1 + ptr..)?;
    if sec.len() < 3 || sec[0] != want_table_id {
        return None;
    }
    let sec_len = (((sec[1] & 0x0F) as usize) << 8) | sec[2] as usize;
    // section_length 含表头后的全部字节（不含 table_id 与 length 自身 3 字节）
    Some(&sec[..(3 + sec_len).min(sec.len())])
}

/// PES 载荷拼装器：剥头 + **按 `PES_packet_length` 截断**。
///
/// 为什么必须截断：TS 包的剩余空间用 `0xFF` 填充（复用、对齐），而一个 PES 的
/// 最后一个包几乎总有填充。整包 append 会把几十到一百多个 `0xFF` 塞进 ADTS
/// 流里——probe 还能过（ADTS 从 0xFFF 同步），解码器却会在尾部拿到一堆假同步字。
/// PES 头里的长度字段是唯一可靠的"数据到哪结束"依据（音频 PES 一定填了它；
/// 为 0 只出现在视频这种长度未知的场景）。
///
/// 跨包续接也靠它：`remaining` 记"这个 PES 还差多少字节"，够了就不再收。
#[derive(Default)]
struct PesAssembler {
    remaining: Option<usize>,
}

impl PesAssembler {
    fn feed(&mut self, payload: &[u8], pusi: bool, out: &mut Vec<u8>) {
        if !pusi {
            // 续包：只收本 PES 还没收够的部分
            let take = match self.remaining {
                Some(r) => r.min(payload.len()),
                None => payload.len(),
            };
            out.extend_from_slice(&payload[..take]);
            if let Some(r) = self.remaining.as_mut() {
                *r -= take;
            }
            return;
        }
        // 起点包：PES 头 = 00 00 01 <stream_id> <len 2B> <flags 2B> <hdr_len 1B> <可选头>
        if payload.len() < 9 || payload[..3] != [0x00, 0x00, 0x01] {
            return; // 头被截断（跨包），等下一个起点
        }
        let pes_len = u16::from_be_bytes([payload[4], payload[5]]) as usize;
        let hdr_len = payload[8] as usize;
        let start = 9 + hdr_len;
        if start > payload.len() {
            return;
        }
        let avail = &payload[start..];
        if pes_len == 0 {
            // 长度未知（视频常见）：能收多少收多少
            out.extend_from_slice(avail);
            self.remaining = None;
            return;
        }
        // PES_packet_length 从 flags 那 3 字节算起，所以数据长度要减掉 3 + 可选头
        let total = pes_len.saturating_sub(3 + hdr_len);
        let take = total.min(avail.len());
        out.extend_from_slice(&avail[..take]);
        self.remaining = Some(total - take);
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// 手拼最小 ADTS 段（与 hls 单测同一个位域，见那里的注释）。
    fn adts_frames(frames: usize, freq_idx: u8, channels: u8) -> Vec<u8> {
        let mut out = Vec::new();
        let frame_len: u16 = 7 + 32;
        for _ in 0..frames {
            let mut h = [0u8; 7];
            h[0] = 0xFF;
            h[1] = 0xF1;
            h[2] = 0x40 | (freq_idx << 2) | ((channels >> 2) & 1);
            h[3] = ((channels & 3) << 6) | ((frame_len >> 11) & 3) as u8;
            h[4] = ((frame_len >> 3) & 0xFF) as u8;
            h[5] = (((frame_len & 7) << 5) | 0x1F) as u8;
            h[6] = 0x3F << 2;
            out.extend_from_slice(&h);
            out.extend_from_slice(&[0x00; 32]);
        }
        out
    }

    /* ---------------- 最小 TS 封装器（反向工具，只给测试用） ---------------- */

    const PID_PMT: u16 = 0x1000;
    const PID_AUDIO: u16 = 0x0101;

    fn packet(pid: u16, pusi: bool, payload: &[u8], cc: u8) -> [u8; PACKET_LEN] {
        let mut p = [0xFFu8; PACKET_LEN];
        p[0] = SYNC;
        p[1] = ((pid >> 8) as u8 & 0x1F) | if pusi { 0x40 } else { 0 };
        p[2] = (pid & 0xFF) as u8;
        // afc=01（纯载荷），continuity counter 占低 4 位
        p[3] = 0x10 | (cc & 0x0F);
        p[4..4 + payload.len()].copy_from_slice(payload);
        p
    }

    /// 一个 PES 包成 N 个 TS 包；`short` 让最后一个包短填充（靠 0xFF 填充）。
    fn pes_packets(pid: u16, stream_id: u8, data: &[u8], cc: &mut u8) -> Vec<[u8; PACKET_LEN]> {
        let mut pes = vec![0x00, 0x00, 0x01, stream_id];
        // PES_packet_length = 3（flags+hdr_len）+ 0（可选头）+ 载荷
        let len = 3 + data.len();
        pes.push(((len >> 8) & 0xFF) as u8);
        pes.push((len & 0xFF) as u8);
        pes.push(0x80); // '10' + 无 PTS/DTS
        pes.push(0x00); // flags
        pes.push(0x00); // header_data_length
        pes.extend_from_slice(data);

        let mut out = Vec::new();
        let mut first = true;
        for chunk in pes.chunks(PACKET_LEN - 4) {
            // 只有首包置 PUSI（`packet()` 按参数置位），其余是续包
            out.push(packet(pid, first, chunk, *cc));
            *cc = cc.wrapping_add(1);
            first = false;
        }
        out
    }

    fn pat_section() -> Vec<u8> {
        let mut s = vec![0x00, 0xB0, 0x0D]; // table_id=0x00, '10'+len=13
        s.extend_from_slice(&[0x00, 0x01]); // transport_stream_id
        s.extend_from_slice(&[0xC1, 0x00, 0x00]); // version/current/seq0/last0
        s.extend_from_slice(&[0x00, 0x01]); // program_number = 1
        s.push(0xE0 | ((PID_PMT >> 8) as u8 & 0x1F));
        s.push((PID_PMT & 0xFF) as u8);
        // 补到 section_length 声明的长度（13 = 3..16）
        while s.len() < 16 {
            s.push(0xFF);
        }
        s
    }

    fn pmt_section(stream_type: u8, es_pid: u16) -> Vec<u8> {
        let mut s = vec![0x02, 0xB0, 0x12]; // table_id=0x02, len=18
        s.extend_from_slice(&[0x00, 0x01]); // program_number
        s.extend_from_slice(&[0xC1, 0x00, 0x00]); // version/seq/last
        s.extend_from_slice(&[0xE0, 0x00]); // PCR_PID（任意）
        s.extend_from_slice(&[0xF0, 0x00]); // program_info_length = 0
        s.push(stream_type);
        s.push(0xE0 | ((es_pid >> 8) as u8 & 0x1F));
        s.push((es_pid & 0xFF) as u8);
        s.extend_from_slice(&[0xF0, 0x00]); // ES_info_length = 0
        while s.len() < 21 {
            s.push(0xFF);
        }
        s
    }

    /// 一个"正常"段：PAT + PMT + 跨多包的 PES。
    pub(crate) fn mux_segment(adts: &[u8], with_tables: bool) -> Vec<u8> {
        let mut out = Vec::new();
        let mut cc = 0u8;
        if with_tables {
            // pointer_field = 0，section 紧跟其后
            let mut pat = vec![0x00];
            pat.extend_from_slice(&pat_section());
            out.extend_from_slice(&packet(PID_PAT, true, &pat, cc));
            cc = cc.wrapping_add(1);
            let mut pmt = vec![0x00];
            pmt.extend_from_slice(&pmt_section(0x0F, PID_AUDIO));
            out.extend_from_slice(&packet(PID_PMT, true, &pmt, cc));
            cc = cc.wrapping_add(1);
        }
        // 每 300 字节一个 PES：保证"一个 PES 跨多个 TS 包"（184 载荷/包）
        for chunk in adts.chunks(300) {
            for p in pes_packets(PID_AUDIO, 0xC0, chunk, &mut cc) {
                out.extend_from_slice(&p);
            }
        }
        out
    }

    /* ---------------- 断言 ---------------- */

    #[test]
    fn roundtrip_adts_through_ts_is_byte_identical() {
        let adts = adts_frames(200, 3, 2);
        let ts = mux_segment(&adts, true);
        assert!(is_ts(&ts), "嗅探认不出自己封的 TS");
        let mut st = TsState::new();
        let (es, kind) = strip(&ts, &mut st).unwrap();
        assert_eq!(kind, TsAudio::Adts);
        assert_eq!(es, adts, "TS 往返必须逐字节相等");
        assert_eq!(st.audio_pid, Some(PID_AUDIO));
    }

    #[test]
    fn segment_without_tables_reuses_cached_pid() {
        let adts = adts_frames(50, 3, 2);
        let first = mux_segment(&adts, true);
        let mut st = TsState::new();
        strip(&first, &mut st).unwrap();

        // 后续段只发 PES：没有 PAT/PMT 也要能剥
        let later = mux_segment(&adts, false);
        let (es, kind) = strip(&later, &mut st).unwrap();
        assert_eq!(kind, TsAudio::Adts);
        assert_eq!(es, adts);
    }

    #[test]
    fn segment_without_tables_and_without_cache_is_unsupported() {
        let adts = adts_frames(5, 3, 2);
        let only_pes = mux_segment(&adts, false);
        let mut st = TsState::new();
        let err = strip(&only_pes, &mut st).unwrap_err();
        assert!(
            matches!(err, AudioError::Unsupported(_)),
            "无节目表要报 Unsupported 而不是 Decode：{err}"
        );
        assert!(err.to_string().contains("PAT/PMT"), "{err}");
    }

    #[test]
    fn mpeg_audio_stream_type_maps_to_mp3_hint() {
        let frames: Vec<u8> = (0..40).flat_map(|_| [0xFFu8, 0xFB, 0x90, 0x00]).collect();
        let mut out = Vec::new();
        let mut cc = 0u8;
        let mut pat = vec![0x00];
        pat.extend_from_slice(&pat_section());
        out.extend_from_slice(&packet(PID_PAT, true, &pat, cc));
        cc = cc.wrapping_add(1);
        let mut pmt = vec![0x00];
        pmt.extend_from_slice(&pmt_section(0x03, PID_AUDIO));
        out.extend_from_slice(&packet(PID_PMT, true, &pmt, cc));
        cc = cc.wrapping_add(1);
        for p in pes_packets(PID_AUDIO, 0xC0, &frames, &mut cc) {
            out.extend_from_slice(&p);
        }
        let mut st = TsState::new();
        let (es, kind) = strip(&out, &mut st).unwrap();
        assert_eq!(kind, TsAudio::MpegAudio);
        assert_eq!(kind.hint_ext(), "mp3");
        assert_eq!(es, frames);
    }

    #[test]
    fn latm_stream_type_is_named_explicitly() {
        let mut out = Vec::new();
        let mut pat = vec![0x00];
        pat.extend_from_slice(&pat_section());
        out.extend_from_slice(&packet(PID_PAT, true, &pat, 0));
        let mut pmt = vec![0x00];
        pmt.extend_from_slice(&pmt_section(0x11, PID_AUDIO));
        out.extend_from_slice(&packet(PID_PMT, true, &pmt, 1));
        out.extend_from_slice(&packet(PID_AUDIO, true, &[0x00; 32], 0));
        let mut st = TsState::new();
        let err = strip(&out, &mut st).unwrap_err();
        assert!(err.to_string().contains("LATM"), "{err}");
    }

    #[test]
    fn truncated_segment_reports_bad_length() {
        let adts = adts_frames(10, 3, 2);
        let mut ts = mux_segment(&adts, true);
        ts.truncate(ts.len() - 7);
        let mut st = TsState::new();
        let err = strip(&ts, &mut st).unwrap_err();
        assert!(err.to_string().contains("188"), "{err}");
    }

    #[test]
    fn lost_sync_reports_packet_index() {
        let adts = adts_frames(10, 3, 2);
        let mut ts = mux_segment(&adts, true);
        // 破坏第二个包的同步字
        ts[PACKET_LEN] = 0x00;
        let mut st = TsState::new();
        let err = strip(&ts, &mut st).unwrap_err();
        assert!(err.to_string().contains("第 2 个包"), "{err}");
    }

    #[test]
    fn adaptation_field_with_payload_is_skipped_correctly() {
        let adts = adts_frames(20, 3, 2);
        let mut out = Vec::new();
        let mut cc = 0u8;
        let mut pat = vec![0x00];
        pat.extend_from_slice(&pat_section());
        out.extend_from_slice(&packet(PID_PAT, true, &pat, cc));
        cc = cc.wrapping_add(1);
        let mut pmt = vec![0x00];
        pmt.extend_from_slice(&pmt_section(0x0F, PID_AUDIO));
        out.extend_from_slice(&packet(PID_PMT, true, &pmt, cc));
        cc = cc.wrapping_add(1);

        // 手工造 afc=0b11 的包：af 长度 5（含 PCR），载荷接在 5+5=10 起
        let mut pes = vec![0x00, 0x00, 0x01, 0xC0];
        let len = 3 + adts.len();
        pes.push(((len >> 8) & 0xFF) as u8);
        pes.push((len & 0xFF) as u8);
        pes.extend_from_slice(&[0x80, 0x00, 0x00]);
        pes.extend_from_slice(&adts);

        let payload_cap = PACKET_LEN - 10;
        let mut first = true;
        for chunk in pes.chunks(payload_cap) {
            let mut p = [0xFFu8; PACKET_LEN];
            p[0] = SYNC;
            p[1] = ((PID_AUDIO >> 8) as u8 & 0x1F) | if first { 0x40 } else { 0 };
            p[2] = (PID_AUDIO & 0xFF) as u8;
            // afc = 0b11（AF + 载荷）；AF 长度 5 含 PCR 占位，所以载荷从 5+5=10 起
            p[3] = 0x30 | (cc & 0x0F);
            p[4] = 5;
            p[10..10 + chunk.len()].copy_from_slice(chunk);
            out.extend_from_slice(&p);
            cc = cc.wrapping_add(1);
            first = false;
        }

        let mut st = TsState::new();
        let (es, _) = strip(&out, &mut st).unwrap();
        assert_eq!(es, adts, "带 adaptation field 的包要正确跳过 AF");
    }

    #[test]
    fn is_ts_rejects_adts_and_short_buffers() {
        assert!(!is_ts(&adts_frames(3, 3, 2)));
        assert!(!is_ts(&[]));
        assert!(!is_ts(&[0x47; 10]));
    }
}
