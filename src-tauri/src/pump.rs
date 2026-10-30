//! 引擎事件泵的纯逻辑核心。
//!
//! 为什么单独抽出来：泵原本整段写在 `spawn` 的 async 闭包里，任何断言都要
//! 起整个 Tauri 运行时（AppHandle、窗口、WebView）。于是"进度节流/过期 token
//! 丢弃/曲尾纠偏下发"这些**最容易写错且错了最不易察觉**的规则，只能靠人肉读
//! 代码保证 —— 上一轮正是这样漏掉了节流基准恒假（进度条全程冻结）。
//!
//! 抽出来后 `PumpCore::ingest` 只有三个输入：一批事件、Actor、当前时刻；
//! 输出是"该做的旁路动作"清单 + 是否动了引擎。时钟与引擎下发出口都可注入，
//! 于是可以在普通 `#[test]` 里断言完整的事件 → 动作序列。

use aiya_audio::{EngineCommand, EngineEvent};
use aiya_player::{PlayerActor, PlayerCommand};

/// 把 Actor 的"装载/停止"意图下发到引擎。
///
/// 三个调用点（命令路径 / 事件泵 / 启动恢复）此前各抄一遍，且都 `let _ = start_ms`
/// 把起点丢弃 —— "记住播放进度"从未生效。顺序固定：
/// SetRate（先落 pending_rate，首帧就用对速率）→ Load(含起点/token/**paused**)
/// → SetVolume。
///
/// `paused` 随 Load 下发（协议 v7）：此前"装载但暂停"要再发一条 Pause，而
/// 引擎在 Load 的开流期间就开始出声、Pause 要等渲染循环第一次收命令才生效
/// ——启动恢复必有几百毫秒残声（用户实测）。现在装载意图自带暂停态，
/// 引擎开流前就知道不该喂数据。
///
/// `send` 是引擎出口（生产环境写进引擎通道，测试里塞进 Vec）。
/// 返回是否真的下了命令（调用方据此决定要不要全量落库）。
pub fn apply_engine_intents<F>(send: &mut F, actor: &mut PlayerActor) -> bool
where
    F: FnMut(EngineCommand),
{
    let mut acted = false;
    if let Some((token, path, start_ms, autoplay)) = actor.take_pending_load() {
        let rate = actor.state().rate as f32;
        // 音量统一由 Actor 算（含 ReplayGain 归一化与静音）：此前这里把
        // `volume, muted` 拆开各算一遍，归一化接进来就会出现第三份口径。
        let volume = actor.engine_volume();
        send(EngineCommand::SetRate(rate));
        send(EngineCommand::Load {
            path,
            start_ms,
            token,
            paused: !autoplay,
        });
        send(EngineCommand::SetVolume(volume));
        acted = true;
    }
    // 队尾停住 / 队列清空：此前只有 pending_load，这两条迁移对引擎完全静默，
    // 表现是 UI 已显示停止而声音继续放到本曲结束。
    if actor.take_pending_stop() {
        send(EngineCommand::Stop);
        acted = true;
    }
    acted
}

/// 泵处理一批事件后需要宿主做的事（按产生顺序返回）。
#[derive(Debug, PartialEq)]
pub enum PumpEffect {
    /// 转发 UI 的频谱帧（`spectrum:frame`）
    Spectrum(Vec<u8>),
    /// 记一条播放历史（seconds 用最近上报位置估算）
    History { track_id: String, seconds: u64 },
    /// 实际音频模式变化（独占被拒回落共享 / 换设备后能力变化）：
    /// 宿主更新诊断状态并转发 `audio:mode`
    /// 输出实际状态快照（模式/时钟/缓冲/格式 + 降级原因）。
    /// 不叫 AudioMode 是为了跟引擎事件区分：这是"给 UI 的最终结论"，
    /// 字段已展开成字符串，UI 不用再认识引擎的枚举。
    OutputMode {
        requested: String,
        mode: String,
        clock: String,
        buffer_ms: u32,
        format: String,
        reason: Option<String>,
    },
    /// 广播 `player:state`（+ SMTC 跟踪）
    EmitState,
}

/// 事件泵的归约状态：两个节流/估算基准**必须是泵私有**。
///
/// `last_emit_ms` 曾经取 `actor.state().updated_at`，而每次归约都会把它置成
/// "此刻"，于是 `now >= updated_at + 500` 恒假 —— 进度条全程不动。
/// `last_position_ms` 是历史秒数的数据源（Eof 时读它，不读整曲时长）。
#[derive(Debug, Default)]
pub struct PumpCore {
    last_position_ms: u64,
    last_emit_ms: u64,
}

impl PumpCore {
    pub fn new() -> Self {
        Self::default()
    }

    /// 处理一批引擎事件：归约进 Actor → 下发引擎意图 → 返回旁路动作。
    ///
    /// 返回 `(effects, acted)`：`acted` = 本批是否实际下发了引擎意图
    /// （pending_load / pending_stop）。广播状态（EmitState）不代表队列变了
    /// ——播放中每 500ms 一次的 Position 节流也置位，调用方落库必须用
    /// `acted` 门控，否则播放全程在反复全量重写 queue_items。
    ///
    /// `now_ms` 由调用方注入（生产用系统时钟，测试用固定值），
    /// 这是让节流可断言的前提。
    pub fn ingest<F>(
        &mut self,
        events: Vec<EngineEvent>,
        actor: &mut PlayerActor,
        now_ms: u64,
        send: &mut F,
    ) -> (Vec<PumpEffect>, bool)
    where
        F: FnMut(EngineCommand),
    {
        let mut effects: Vec<PumpEffect> = Vec::new();
        let mut should_emit = false;

        for ev in events {
            match ev {
                EngineEvent::Loaded { token, duration_ms } => {
                    // token 来自引擎回显，不再用"当前 token"顶替：
                    // 过期的 Loaded 会被 Actor 丢弃（不会把旧曲时长写到刚切过去的新曲上）
                    let live = actor.state().token;
                    actor.reduce(PlayerCommand::Loaded { token, duration_ms });
                    if token == live {
                        should_emit = true;
                    }
                }
                EngineEvent::Position { token, position_ms } => {
                    // 过期曲目的位置直接丢：否则切歌瞬间旧曲的进度会打到新曲上
                    if token != actor.state().token {
                        continue;
                    }
                    // 全量记录（历史秒数用），广播才节流
                    self.last_position_ms = position_ms;
                    if position_emit_due(now_ms, self.last_emit_ms) {
                        self.last_emit_ms = now_ms;
                        actor.reduce(PlayerCommand::Position { position_ms });
                        should_emit = true;
                    }
                }
                EngineEvent::Eof { token } => {
                    // 同样按 token 归属：曲尾按"下一首"时旧曲的 Eof 不能记到
                    // 新曲头上（会连跳两首）
                    if token != actor.state().token {
                        continue;
                    }
                    // seconds_played 用最近上报位置估算（超出的按总时长截断；
                    // 10 秒就跳过的歌不算听完）
                    let (tid, secs) = {
                        let s = actor.state();
                        (
                            s.current.as_ref().map(|c| c.id.clone()),
                            s.duration_ms.unwrap_or(0) / 1000,
                        )
                    };
                    let secs = secs.min(self.last_position_ms / 1000);
                    if let Some(track_id) = tid {
                        effects.push(PumpEffect::History {
                            track_id,
                            seconds: secs,
                        });
                    }
                    actor.reduce(PlayerCommand::Ended { token });
                    self.last_position_ms = 0;
                    self.last_emit_ms = 0;
                    should_emit = true;
                }
                EngineEvent::StateChanged { .. } => {
                    // Pause/Resume 由命令路径处理；这里对齐引擎侧被动变化
                }
                EngineEvent::Spectrum { bands } => {
                    // 频谱帧：不经 Actor，锁外统一转发 UI（~30fps）
                    effects.push(PumpEffect::Spectrum(bands));
                }
                EngineEvent::AudioMode {
                    requested,
                    mode,
                    clock,
                    buffer_ms,
                    format,
                    reason,
                    ..
                } => {
                    // 模式是**设备/设置**的属性，与曲目身份无关：不按 token 过滤
                    // （设备恢复导致降级时，token 往往还是当前曲目，过滤反而漏报）
                    effects.push(PumpEffect::OutputMode {
                        requested,
                        mode,
                        clock,
                        buffer_ms,
                        format,
                        reason,
                    });
                }
                EngineEvent::Error {
                    token,
                    kind,
                    message,
                } => {
                    // token=0 表示"装载前的错误"，仍要上报
                    if token != 0 && token != actor.state().token {
                        continue;
                    }
                    // 分类由引擎在错误产生处给出（v5 协议），宿主只做翻译，
                    // 绝不解析消息文案——文案是会改的，分类不会。
                    actor.reduce(PlayerCommand::Failed {
                        error: aiya_player::AudioError::new(
                            aiya_player::AudioErrorCode::parse(&kind),
                            message,
                        ),
                    });
                    should_emit = true;
                }
                EngineEvent::EqApplied { .. } => {
                    // EQ 生效确认（v6）。生效值随 player_set_eq 的返回值到 UI，
                    // 恢复场景（sidecar 重启回放）下 UI 设置没变，无需转发
                }
            }
        }

        // 自然播完/队尾停住都会产出意图（pending_load 或 pending_stop）；
        // 与命令路径共用同一套下发顺序。
        let acted = apply_engine_intents(send, actor);
        if acted {
            // 换了曲：位置与节流基准都从新曲起点重算（否则新曲首帧可能被
            // 上一首的节流基准压掉，UI 短暂显示上一首的进度）
            self.last_position_ms = actor.state().position_ms;
            self.last_emit_ms = 0;
        }

        if should_emit {
            effects.push(PumpEffect::EmitState);
        }
        (effects, acted)
    }
}

/// 进度广播间隔（毫秒）：引擎每 ~10ms 报一次位置，UI 只需要 2Hz。
pub const POSITION_EMIT_INTERVAL_MS: u64 = 500;

/// 位置广播节流。
///
/// 抽成纯函数是因为它错过一次：基准取 `updated_at`（每次归约都被置成"此刻"）
/// 导致条件恒假。用 `saturating_add` 让时钟回拨/极大值不 panic。
pub fn position_emit_due(now_ms: u64, last_emit_ms: u64) -> bool {
    now_ms >= last_emit_ms.saturating_add(POSITION_EMIT_INTERVAL_MS)
}

#[cfg(test)]
mod tests {
    use super::*;
    use aiya_player::{PlaybackStatus, QueueOp, TrackRef};

    fn track(id: &str) -> TrackRef {
        TrackRef {
            id: id.to_string(),
            title: id.to_string(),
            artist: None,
            album: None,
            artist_id: None,
            album_id: None,
            duration_ms: Some(10_000),
            artwork_id: None,
            gain_db: None,
            audio: None,
            path: format!("D:/music/{id}.flac"),
        }
    }

    /// 造一个"两首队列、正在播第一首"的 Actor。
    /// ReplaceAll 会产出 pending_load（token=1），先取走丢掉 ——
    /// 测试关心的是之后的事件流。
    fn playing_two() -> PlayerActor {
        let mut actor = PlayerActor::new();
        actor.reduce(PlayerCommand::Queue(QueueOp::ReplaceAll {
            tracks: vec![track("a"), track("b")],
            start: 0,
        }));
        let _ = actor.take_pending_load();
        actor
    }

    fn cmds_for(actor: &mut PlayerActor) -> Vec<EngineCommand> {
        let mut out = Vec::new();
        apply_engine_intents(&mut |c| out.push(c), actor);
        out
    }

    /// 协议 v7：暂停态随 Load 下发，不再有独立的 Pause 命令——
    /// 启动恢复不能"先响一下"（引擎在开流期间就开始出声，Pause 迟到）。
    #[test]
    fn load_carries_paused_flag_without_extra_pause() {
        let mut actor = PlayerActor::new();
        // 播放路径（ReplaceAll 默认自动播放）
        actor.reduce(PlayerCommand::Queue(QueueOp::ReplaceAll {
            tracks: vec![track("x")],
            start: 0,
        }));
        let cmds = cmds_for(&mut actor);
        assert!(
            matches!(&cmds[1], EngineCommand::Load { paused: false, .. }),
            "播放路径装载不打暂停: {cmds:?}"
        );
        assert!(!cmds.contains(&EngineCommand::Pause));

        // hydrate：装载但暂停（instruct_load(false, _)，启动恢复路径）
        let mut actor = PlayerActor::new();
        actor.hydrate(
            vec![track("x")],
            aiya_player::RepeatMode::Off,
            false,
            0.8,
            false,
            1.0,
            5_000,
            Some("x"),
        );
        let cmds = cmds_for(&mut actor);
        assert!(
            matches!(&cmds[1], EngineCommand::Load { paused: true, .. }),
            "暂停装载必须带 paused:true: {cmds:?}"
        );
        assert!(
            !cmds.contains(&EngineCommand::Pause),
            "暂停态不单独发 Pause"
        );
    }

    #[test]
    fn load_volume_carries_replaygain() {
        // 归一化开 + gain_db=-6：装载下发的 SetVolume 必须是 0.85×0.501，
        // 否则"归一化"只体现在 UI 上、耳朵听不到
        let mut actor = PlayerActor::new();
        let mut t = track("x");
        t.gain_db = Some(-6.0);
        actor.set_normalize(true);
        actor.reduce(PlayerCommand::Queue(QueueOp::ReplaceAll {
            tracks: vec![t],
            start: 0,
        }));
        let cmds = cmds_for(&mut actor);
        let vol = cmds
            .iter()
            .find_map(|c| match c {
                EngineCommand::SetVolume(v) => Some(*v),
                _ => None,
            })
            .expect("装载序列必须带 SetVolume");
        assert!((vol - 0.85 * 0.5011872).abs() < 1e-4, "实际 {vol}");
    }

    #[test]
    fn load_volume_respects_mute() {
        let mut actor = PlayerActor::new();
        actor.reduce(PlayerCommand::SetVolume {
            volume: 0.8,
            muted: true,
        });
        actor.reduce(PlayerCommand::Queue(QueueOp::ReplaceAll {
            tracks: vec![track("x")],
            start: 0,
        }));
        let cmds = cmds_for(&mut actor);
        assert!(
            cmds.contains(&EngineCommand::SetVolume(0.0)),
            "静音装载必须下发 0"
        );
    }

    /* ---------------- 节流 ---------------- */

    #[test]
    fn position_emit_due_is_500ms_window() {
        assert!(position_emit_due(1_000, 500)); // 正好 500ms → 放行
        assert!(!position_emit_due(999, 500)); // 差 1ms → 压掉
                                               // 时钟回拨（now < last）不 panic、也不误放开：saturating 兜住溢出，
                                               // 结果维持"不广播"，等时间追上再说
        assert!(!position_emit_due(0, u64::MAX));
    }

    #[test]
    fn positions_are_throttled_to_one_emit_per_window() {
        let mut actor = playing_two();
        let mut core = PumpCore::new();
        let mut sent = Vec::new();
        let token = actor.state().token;

        // 引擎 ~10ms 一帧；同一 500ms 窗口内只有第一帧该广播
        let mut emits = 0usize;
        for i in 1..=10u64 {
            let (fx, acted) = core.ingest(
                vec![EngineEvent::Position {
                    token,
                    position_ms: i * 10,
                }],
                &mut actor,
                1_000 + i * 10,
                &mut |c| sent.push(c),
            );
            emits += fx.iter().filter(|e| **e == PumpEffect::EmitState).count();
            assert!(!acted, "Position tick 只广播状态，不报告动了引擎");
        }
        assert_eq!(emits, 1, "10 帧 100ms 内只该广播一次");
        // 位置本身仍然逐帧记下（Eof 的 seconds_played 靠它）
        assert_eq!(core.last_position_ms, 100);
        assert_eq!(actor.state().position_ms, 10, "被节流的帧不进 Actor");
    }

    #[test]
    fn next_window_emits_again() {
        let mut actor = playing_two();
        let mut core = PumpCore::new();
        let mut sent = Vec::new();
        let token = actor.state().token;
        let mut emits = 0usize;
        for (i, now) in [1_000u64, 1_100, 1_600].into_iter().enumerate() {
            let (fx, acted) = core.ingest(
                vec![EngineEvent::Position {
                    token,
                    position_ms: (i as u64 + 1) * 100,
                }],
                &mut actor,
                now,
                &mut |c| sent.push(c),
            );
            emits += fx.iter().filter(|e| **e == PumpEffect::EmitState).count();
            assert!(!acted);
        }
        assert_eq!(emits, 2, "1000 放行、1100 压掉、1600 放行");
    }

    /* ---------------- token 过滤 ---------------- */

    #[test]
    fn stale_position_and_eof_are_dropped() {
        let mut actor = playing_two();
        let mut core = PumpCore::new();
        let mut sent = Vec::new();
        let stale = actor.state().token; // 当前曲 token
                                         // 用户按"下一首"：Actor 换代到 token+1，产生 pending_load
        actor.reduce(PlayerCommand::Next);
        let new_token = actor.state().token;
        assert_ne!(new_token, stale);
        let cmds = cmds_for(&mut actor);
        assert!(matches!(cmds[0], EngineCommand::SetRate(_)));
        assert!(matches!(cmds[1], EngineCommand::Load { token, .. } if token == new_token));

        // 旧曲的 Position / Eof 迟到 → 全丢，不能记到新曲头上
        let (fx, acted) = core.ingest(
            vec![
                EngineEvent::Position {
                    token: stale,
                    position_ms: 9_000,
                },
                EngineEvent::Eof { token: stale },
            ],
            &mut actor,
            2_000,
            &mut |c: EngineCommand| sent.push(c),
        );
        assert!(fx.is_empty(), "过期 token 不该产生任何动作: {fx:?}");
        assert!(!acted, "全被丢弃的一批不该报告动了引擎");
        assert_eq!(actor.state().position_ms, 0, "新曲位置未被旧曲覆盖");
        assert_eq!(actor.state().token, new_token, "过期 Eof 不该推进队列");
    }

    /* ---------------- 曲尾：记历史 + 纠偏下发 ---------------- */

    #[test]
    fn eof_records_history_with_played_seconds_and_stops_at_tail() {
        let mut actor = PlayerActor::new();
        actor.reduce(PlayerCommand::Queue(QueueOp::ReplaceAll {
            tracks: vec![track("only")],
            start: 0,
        }));
        let token = actor.state().token;
        let _ = actor.take_pending_load();
        let mut core = PumpCore::new();
        let mut sent = Vec::new();

        // 播到 4.2s 就曲终（duration 10s）→ 历史记 4s，不是整曲 10s
        let (fx, acted) = core.ingest(
            vec![EngineEvent::Position {
                token,
                position_ms: 4_200,
            }],
            &mut actor,
            5_000,
            &mut |c| sent.push(c),
        );
        assert!(!acted, "Position tick 不落库");
        assert!(
            fx.contains(&PumpEffect::EmitState),
            "位置广播照常（节流窗口起点）"
        );
        let (fx, acted) = core.ingest(
            vec![EngineEvent::Eof { token }],
            &mut actor,
            5_100,
            &mut |c| sent.push(c),
        );
        assert!(acted, "曲尾推进队列（换曲/停止）必须报告 acted → 全量落库");
        assert_eq!(
            fx,
            vec![
                PumpEffect::History {
                    track_id: "only".into(),
                    seconds: 4
                },
                PumpEffect::EmitState,
            ]
        );
        // 队尾：状态回 Idle 且必须下发 Stop（否则引擎继续把这首放完）
        assert_eq!(actor.state().status, PlaybackStatus::Idle);
        assert_eq!(sent, vec![EngineCommand::Stop]);
        assert_eq!(core.last_position_ms, 0, "曲终清零，下一首重新估算");
    }

    #[test]
    fn eof_mid_queue_loads_next_with_autoplay() {
        let mut actor = playing_two();
        let mut core = PumpCore::new();
        let token = actor.state().token;
        let mut sent = Vec::new();
        let (fx, acted) = core.ingest(
            vec![EngineEvent::Eof { token }],
            &mut actor,
            1_000,
            &mut |c| sent.push(c),
        );
        assert!(fx.contains(&PumpEffect::EmitState));
        assert!(acted, "自动换曲该报告 acted");
        // 自动换曲：SetRate → Load(第二首, 起点 0, 新 token) → SetVolume
        match &sent[..] {
            [EngineCommand::SetRate(_), EngineCommand::Load {
                path,
                start_ms,
                token: t,
                ..
            }, EngineCommand::SetVolume(_)] => {
                assert_eq!(path, "D:/music/b.flac");
                assert_eq!(*start_ms, 0);
                assert_eq!(*t, token + 1);
            }
            other => panic!("自动换曲指令序列不对: {other:?}"),
        }
    }

    /* ---------------- 频谱 / 错误 ---------------- */

    #[test]
    fn spectrum_frames_pass_through_untouched() {
        let mut actor = playing_two();
        let mut core = PumpCore::new();
        let mut sent = Vec::new();
        let (fx, acted) = core.ingest(
            vec![
                EngineEvent::Spectrum {
                    bands: vec![1, 2, 3],
                },
                EngineEvent::Spectrum {
                    bands: vec![4, 5, 6],
                },
            ],
            &mut actor,
            1_000,
            &mut |c| sent.push(c),
        );
        assert_eq!(
            fx,
            vec![
                PumpEffect::Spectrum(vec![1, 2, 3]),
                PumpEffect::Spectrum(vec![4, 5, 6])
            ]
        );
        assert!(!fx.contains(&PumpEffect::EmitState), "频谱不触发状态广播");
        assert!(!acted);
    }

    #[test]
    fn error_without_token_still_reports() {
        let mut actor = playing_two();
        let mut core = PumpCore::new();
        let mut sent = Vec::new();
        // token=0：装载前的错误（引擎侧开流失败），必须上报给 UI
        let (fx, acted) = core.ingest(
            vec![EngineEvent::Error {
                token: 0,
                kind: "deviceLost".into(),
                message: "device lost".into(),
            }],
            &mut actor,
            1_000,
            &mut |c| sent.push(c),
        );
        assert_eq!(fx, vec![PumpEffect::EmitState]);
        assert!(!acted, "失败态不产生引擎意图，不触发全量落库");
        assert_eq!(actor.state().status, PlaybackStatus::Error);
        let err = actor.state().error.clone().expect("错误必须被记录");
        assert_eq!(err.code, aiya_player::AudioErrorCode::DeviceLost);
        assert_eq!(err.message, "device lost");
    }

    #[test]
    fn stale_error_is_dropped() {
        let mut actor = playing_two();
        let mut core = PumpCore::new();
        let mut sent = Vec::new();
        let stale = actor.state().token;
        actor.reduce(PlayerCommand::Next);
        let (fx, _acted) = core.ingest(
            vec![EngineEvent::Error {
                token: stale,
                kind: "decode".into(),
                message: "old".into(),
            }],
            &mut actor,
            1_000,
            &mut |c| sent.push(c),
        );
        assert!(fx.is_empty(), "过期 token 的错误不该污染新曲状态");
    }

    /* ---------------- 装载回报 ---------------- */

    #[test]
    fn loaded_with_stale_token_does_not_emit() {
        let mut actor = playing_two();
        let mut core = PumpCore::new();
        let mut sent = Vec::new();
        let stale = actor.state().token;
        actor.reduce(PlayerCommand::Next); // 换代
        let (fx, _acted) = core.ingest(
            vec![EngineEvent::Loaded {
                token: stale,
                duration_ms: 999_999,
            }],
            &mut actor,
            1_000,
            &mut |c| sent.push(c),
        );
        assert!(fx.is_empty(), "旧曲的 Loaded 不广播");
        assert_ne!(
            actor.state().duration_ms,
            Some(999_999),
            "更不该把旧曲时长写到新曲上"
        );
    }

    /* ---------------- 落库门控（P1 回归） ---------------- */

    /// 泵循环用 EmitState 门控全量落库的旧缺陷：播放中每 500ms 一次的
    /// Position 节流也置位 EmitState，于是 queue_items 被反复 DELETE+重写，
    /// 且全程持有 player 锁。acted 必须对 Position tick 恒假。
    #[test]
    fn position_ticks_never_report_acted() {
        let mut actor = playing_two();
        let mut core = PumpCore::new();
        let token = actor.state().token;
        // 跨多个节流窗口（500ms 一放行），每个窗口都不该报告 acted
        for now in [1_000u64, 1_600, 2_200] {
            let (fx, acted) = core.ingest(
                vec![EngineEvent::Position {
                    token,
                    position_ms: 100,
                }],
                &mut actor,
                now,
                &mut |_| {},
            );
            assert!(fx.contains(&PumpEffect::EmitState), "{now} 应广播状态");
            assert!(!acted, "{now} 的 Position tick 不得触发队列落库");
        }
    }

    /// 真实的队列变更（曲尾换曲）必须报告 acted：否则播放器重启后
    /// 队列停在旧曲上。与上一条互为正反两例。
    #[test]
    fn eof_queue_change_reports_acted() {
        let mut actor = playing_two();
        let mut core = PumpCore::new();
        let token = actor.state().token;
        let (_, acted) = core.ingest(
            vec![EngineEvent::Eof { token }],
            &mut actor,
            1_000,
            &mut |_| {},
        );
        assert!(acted, "曲尾自动换曲必须落库");
        assert_eq!(actor.state().token, token + 1);
    }
}
