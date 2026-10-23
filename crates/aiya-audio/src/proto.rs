//! sidecar 线协议（ADR-0002）。
//!
//! - 传输：命名管道，UTF-8 JSON 行帧（每条消息一行，`\n` 结尾）
//! - 只传命令/事件/心跳，不传 PCM
//! - `PROTOCOL_VERSION` 握手：不兼容拒绝连接，不静默降级
//! - sidecar 不打开 SQLite、不依赖 Tauri 类型

use serde::{Deserialize, Serialize};

/// 协议版本。不匹配的握手直接失败，由调用方决定重启或退出。
pub const PROTOCOL_VERSION: u32 = 1;

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
        for m in [
            HostToSidecar::Hello { protocol: PROTOCOL_VERSION },
            HostToSidecar::Ping,
            HostToSidecar::Cmd(EngineCommand::Load("D:/m/a.flac".into())),
            HostToSidecar::Cmd(EngineCommand::SetVolume(0.42)),
            HostToSidecar::Cmd(EngineCommand::Seek(12_345)),
            HostToSidecar::Cmd(EngineCommand::SetRate(1.25)),
            HostToSidecar::Cmd(EngineCommand::Pause),
            HostToSidecar::Cmd(EngineCommand::Stop),
        ] {
            assert_eq!(format!("{:?}", roundtrip_in(m.clone())), format!("{:?}", m));
        }
        for m in [
            SidecarToHost::Welcome { protocol: PROTOCOL_VERSION },
            SidecarToHost::Pong,
            SidecarToHost::Hb,
            SidecarToHost::Evt(EngineEvent::Loaded { duration_ms: 5000 }),
            SidecarToHost::Evt(EngineEvent::Position { position_ms: 1234 }),
            SidecarToHost::Evt(EngineEvent::Eof),
            SidecarToHost::Evt(EngineEvent::StateChanged { playing: true }),
            SidecarToHost::Evt(EngineEvent::Error { message: "boom".into() }),
        ] {
            assert_eq!(format!("{:?}", roundtrip_out(m.clone())), format!("{:?}", m));
        }
    }

    #[test]
    fn wire_shapes_are_stable() {
        // 形状锚点：协议字段改名会让旧 sidecar/host 互相对不上，必须显式发现
        let hello = encode_line(&HostToSidecar::Hello { protocol: 1 }).unwrap();
        assert_eq!(std::str::from_utf8(&hello).unwrap(), "{\"k\":\"hello\",\"m\":{\"protocol\":1}}\n");

        let load = encode_line(&HostToSidecar::Cmd(EngineCommand::Load("p".into()))).unwrap();
        assert_eq!(
            std::str::from_utf8(&load).unwrap(),
            "{\"k\":\"cmd\",\"m\":{\"t\":\"load\",\"v\":\"p\"}}\n"
        );

        let pos =
            encode_line(&SidecarToHost::Evt(EngineEvent::Position { position_ms: 7 })).unwrap();
        assert_eq!(
            std::str::from_utf8(&pos).unwrap(),
            "{\"k\":\"evt\",\"m\":{\"t\":\"position\",\"positionMs\":7}}\n"
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
