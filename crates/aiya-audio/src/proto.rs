//! sidecar 线协议（ADR-0002）。
//!
//! - 传输：命名管道，UTF-8 JSON 行帧（每条消息一行，`\n` 结尾）
//! - 只传命令/事件/心跳，不传 PCM
//! - `PROTOCOL_VERSION` 握手：不兼容拒绝连接，不静默降级
//! - sidecar 不打开 SQLite、不依赖 Tauri 类型

use serde::{Deserialize, Serialize};

/// 协议版本。不匹配的握手直接失败，由调用方决定重启或退出。
///
/// v6：新增 `SetEq(EqParams)` 命令与 `EngineEvent::EqApplied`（docs/24 B5b，
/// 10 段 biquad）。EQ 是设置而非曲目属性：恢复序列在 SetOutputConfig 之后
/// 无条件回放。`EqApplied` 带生效值（clamp 后），UI 以它为准而非自己发的值。
///
/// v5：`EngineEvent::Error` 加 `kind` 分类（`fileMissing`/`io`/`decode`/
/// `codecUnsupported`/`deviceLost`/`unsupportedFormat`/`unknown`）。此前只有
/// 一句话，宿主无法区分"文件没了该跳过"和"设备断了该提示"，只能解析文案前缀。
/// v5.1（无版本号递增的**加值**）：kind 新增 `codecUnsupported`（docs/22 G1，
/// 容器认识但编码没编进 features）。旧宿主 parse 不认识会落到 `unknown`，
/// 向后兼容，故不 bump 协议版本。
///
/// v4：`SetExclusive(bool)` → `SetOutputConfig(OutputConfig)`（模式/时钟/缓冲/
///     暂停释放一组下发）；`AudioMode` 事件从"只有 exclusive 布尔"扩成
///     `requested + mode + clock + bufferMs + format + reason` 的完整快照——
///     用户需要知道"我选的独占·事件·10ms，设备实际给了什么"。
/// v3：新增 `SetExclusive` 命令与 `AudioMode` 事件（独占可选 + 失败可解释）。
/// v2：`Load` 携带 `start_ms` + `token`；所有事件回显 `token`
/// （ADR-0003 generation，修"启动恢复从 0 播"与"曲尾切歌连跳两首"）。
pub const PROTOCOL_VERSION: u32 = 6;

/// host → sidecar 消息。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "k", content = "m", rename_all = "camelCase")]
pub enum HostToSidecar {
    /// 握手：host 声明自己说的协议版本
    Hello { protocol: u32 },
    /// 应用层心跳探测（sidecar 自身另有 1s Hb 下行）
    Ping,
    /// 引擎命令
    Cmd(crate::EngineCommand),
}

/// sidecar → host 消息。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "k", content = "m", rename_all = "camelCase")]
pub enum SidecarToHost {
    /// 握手应答：sidecar 声明协议版本
    Welcome { protocol: u32 },
    /// Ping 的应答
    Pong,
    /// 存活心跳（sidecar 每 1s 发；任何下行消息都可刷新 host 的存活时钟）
    Hb,
    /// 引擎事件
    Evt(crate::EngineEvent),
}

/// JSON 行帧编码：一行 + `\n`。
pub fn encode_line<T: Serialize>(msg: &T) -> serde_json::Result<Vec<u8>> {
    let mut buf = serde_json::to_vec(msg)?;
    buf.push(b'\n');
    Ok(buf)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{EngineCommand, EngineEvent};

    fn roundtrip_in(msg: HostToSidecar) -> HostToSidecar {
        let bytes = encode_line(&msg).unwrap();
        assert_eq!(bytes.last(), Some(&b'\n'), "行帧以 \\n 结尾");
        let line = std::str::from_utf8(&bytes[..bytes.len() - 1]).unwrap();
        serde_json::from_str(line).unwrap()
    }

    fn roundtrip_out(msg: SidecarToHost) -> SidecarToHost {
        let bytes = encode_line(&msg).unwrap();
        let line = std::str::from_utf8(&bytes[..bytes.len() - 1]).unwrap();
        serde_json::from_str(line).unwrap()
    }

    #[test]
    fn wire_roundtrips() {
        let load = EngineCommand::Load { path: "D:/m/a.flac".into(), start_ms: 0, token: 1 };
        for m in [
            HostToSidecar::Hello { protocol: PROTOCOL_VERSION },
            HostToSidecar::Ping,
            HostToSidecar::Cmd(load),
            HostToSidecar::Cmd(EngineCommand::SetVolume(0.42)),
            HostToSidecar::Cmd(EngineCommand::Seek(12_345)),
            HostToSidecar::Cmd(EngineCommand::SetRate(1.25)),
            HostToSidecar::Cmd(EngineCommand::Pause),
            HostToSidecar::Cmd(EngineCommand::Stop),
            HostToSidecar::Cmd(EngineCommand::SetOutputConfig(Default::default())),
            HostToSidecar::Cmd(EngineCommand::SetEq(crate::eq::EqParams::default())),
        ] {
            assert_eq!(format!("{:?}", roundtrip_in(m.clone())), format!("{:?}", m));
        }
        for m in [
            SidecarToHost::Welcome { protocol: PROTOCOL_VERSION },
            SidecarToHost::Pong,
            SidecarToHost::Hb,
            SidecarToHost::Evt(EngineEvent::Loaded { token: 1, duration_ms: 5000 }),
            SidecarToHost::Evt(EngineEvent::Position { token: 1, position_ms: 1234 }),
            SidecarToHost::Evt(EngineEvent::Eof { token: 1 }),
            SidecarToHost::Evt(EngineEvent::StateChanged { token: 1, playing: true }),
            SidecarToHost::Evt(EngineEvent::Error {
                token: 1,
                kind: "decode".into(),
                message: "boom".into(),
            }),
            SidecarToHost::Evt(EngineEvent::EqApplied {
                token: 1,
                params: crate::eq::EqParams::default(),
            }),
            SidecarToHost::Evt(EngineEvent::AudioMode {
                token: 1,
                requested: "exclusive".into(),
                mode: "shared".into(),
                clock: "events".into(),
                buffer_ms: 200,
                format: "f32".into(),
                reason: Some("设备不支持".into()),
            }),
        ] {
            assert_eq!(format!("{:?}", roundtrip_out(m.clone())), format!("{:?}", m));
        }
    }

    #[test]
    fn wire_shapes_are_stable() {
        // 形状锚点：协议字段改名会让旧 sidecar/host 互相对不上，必须显式发现
        let hello = encode_line(&HostToSidecar::Hello { protocol: 4 }).unwrap();
        assert_eq!(std::str::from_utf8(&hello).unwrap(), "{\"k\":\"hello\",\"m\":{\"protocol\":4}}\n");

        let load = encode_line(&HostToSidecar::Cmd(EngineCommand::Load {
            path: "p".into(),
            start_ms: 0,
            token: 7,
        }))
        .unwrap();
        assert_eq!(
            std::str::from_utf8(&load).unwrap(),
            "{\"k\":\"cmd\",\"m\":{\"t\":\"load\",\"v\":{\"path\":\"p\",\"startMs\":0,\"token\":7}}}\n"
        );

        let pos = encode_line(&SidecarToHost::Evt(EngineEvent::Position {
            token: 3,
            position_ms: 7,
        }))
        .unwrap();
        assert_eq!(
            std::str::from_utf8(&pos).unwrap(),
            "{\"k\":\"evt\",\"m\":{\"t\":\"position\",\"token\":3,\"positionMs\":7}}\n"
        );

        // v4 形状：配置整体下发（整体下发才能真正做到"一次改完只重建一次流"）
        let cfg = encode_line(&HostToSidecar::Cmd(EngineCommand::SetOutputConfig(
            crate::OutputConfig {
                mode: crate::ModePref::ExclusiveRequired,
                buffer_ms: 50,
                clock: crate::ClockPref::Polling,
                release_on_pause: true,
            },
        )))
        .unwrap();
        assert_eq!(
            std::str::from_utf8(&cfg).unwrap(),
            "{\"k\":\"cmd\",\"m\":{\"t\":\"setOutputConfig\",\"v\":{\"mode\":\"exclusiveRequired\",\"bufferMs\":50,\"clock\":\"polling\",\"releaseOnPause\":true}}}\n"
        );

        // 事件是完整快照：requested 与 mode 必须能同时出现（回落时两者不同）
        let mode = encode_line(&SidecarToHost::Evt(EngineEvent::AudioMode {
            token: 4,
            requested: "exclusive".into(),
            mode: "shared".into(),
            clock: "events".into(),
            buffer_ms: 200,
            format: "f32".into(),
            reason: Some("设备拒了".into()),
        }))
        .unwrap();
        assert_eq!(
            std::str::from_utf8(&mode).unwrap(),
            "{\"k\":\"evt\",\"m\":{\"t\":\"audioMode\",\"token\":4,\"requested\":\"exclusive\",\"mode\":\"shared\",\"clock\":\"events\",\"bufferMs\":200,\"format\":\"f32\",\"reason\":\"设备拒了\"}}\n"
        );

        // v5 形状：错误必须自带分类，UI 才能决定"跳过"还是"提示"
        let err = encode_line(&SidecarToHost::Evt(EngineEvent::Error {
            token: 5,
            kind: "fileMissing".into(),
            message: "io: 系统找不到指定的文件。".into(),
        }))
        .unwrap();
        assert_eq!(
            std::str::from_utf8(&err).unwrap(),
            "{\"k\":\"evt\",\"m\":{\"t\":\"error\",\"token\":5,\"kind\":\"fileMissing\",\"message\":\"io: 系统找不到指定的文件。\"}}\n"
        );

        // v6 形状：EQ 整体下发 + 生效值回报（camelCase 字段，bands 是数组）
        let eq = encode_line(&HostToSidecar::Cmd(EngineCommand::SetEq(crate::eq::EqParams {
            bands: [6.0, 0.0, 0.0, 0.0, 0.0, -2.0, 0.0, 0.0, 0.0, 3.0],
            pregain: -3.0,
            enabled: true,
        })))
        .unwrap();
        assert_eq!(
            std::str::from_utf8(&eq).unwrap(),
            "{\"k\":\"cmd\",\"m\":{\"t\":\"setEq\",\"v\":{\"bands\":[6.0,0.0,0.0,0.0,0.0,-2.0,0.0,0.0,0.0,3.0],\"pregain\":-3.0,\"enabled\":true}}}\n"
        );
        let applied = encode_line(&SidecarToHost::Evt(EngineEvent::EqApplied {
            token: 7,
            params: crate::eq::EqParams { bands: [6.0, 0.0, 0.0, 0.0, 0.0, -2.0, 0.0, 0.0, 0.0, 3.0], pregain: -3.0, enabled: true },
        }))
        .unwrap();
        assert_eq!(
            std::str::from_utf8(&applied).unwrap(),
            "{\"k\":\"evt\",\"m\":{\"t\":\"eqApplied\",\"token\":7,\"params\":{\"bands\":[6.0,0.0,0.0,0.0,0.0,-2.0,0.0,0.0,0.0,3.0],\"pregain\":-3.0,\"enabled\":true}}}\n"
        );
    }

    #[test]
    fn incompatible_protocol_is_detectable() {
        // host 发 v99，sidecar 侧校验逻辑：版本不等 → 拒绝
        let msg: HostToSidecar =
            serde_json::from_str("{\"k\":\"hello\",\"m\":{\"protocol\":99}}").unwrap();
        match msg {
            HostToSidecar::Hello { protocol } => assert_ne!(protocol, PROTOCOL_VERSION),
            _ => panic!("应为 Hello"),
        }
    }
}
