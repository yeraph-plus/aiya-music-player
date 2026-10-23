//! PlayerActor：播放状态机唯一真相。
//!
//! 纯逻辑（不碰线程/引擎）：命令进 → 状态变 + 零或一跳出 → 音频出口指令出。
//! 引擎适配层（src-tauri）负责把跳出的指令翻译成 aiya-audio 的 EngineCommand，
//! 并把 EngineEvent 反喂成 PlayerCommand。
//!
//! 顺序模型（对齐 contracts）：`queue` 是原序，`order` 是播放顺序
//! （shuffle 开启时打乱）。`cursor` 在 order 里走，`index` 是 order[cursor]。

use crate::state::{
    PlaybackStatus, PlayerCommand, PlayerState, QueueOp, QueueSnapshot, RepeatMode, TrackRef,
};

pub struct PlayerActor {
    queue: Vec<TrackRef>,
    order: Vec<usize>,
    cursor: usize,
    repeat: RepeatMode,
    shuffle: bool,
    status: PlaybackStatus,
    position_ms: u64,
    duration_ms: Option<u64>,
    volume: f64,
    muted: bool,
    rate: f64,
    error: Option<String>,
    queue_version: u64,
    token: u64,
    /// 最近一次需要发出的引擎指令（Actor 不直接驱动线程）
    pending_load: Option<(u64, String, u64, bool)>, // (token, path, startMs, autoplay)
    /// Actor 自己维护的单调位置估计（无引擎回报时给 UI）
    last_reported_ms: u64,
}

impl Default for PlayerActor {
    fn default() -> Self {
        Self::new()
    }
}

impl PlayerActor {
    pub fn new() -> Self {
        Self {
            queue: Vec::new(),
            order: Vec::new(),
            cursor: 0,
            repeat: RepeatMode::Off,
            shuffle: false,
            status: PlaybackStatus::Idle,
            position_ms: 0,
            duration_ms: None,
            volume: 0.85,
            muted: false,
            rate: 1.0,
            error: None,
            queue_version: 0,
            token: 0,
            pending_load: None,
            last_reported_ms: 0,
        }
    }

    /* ---------------- 对外查询 ---------------- */

    pub fn state(&self) -> PlayerState {
        let index = if self.queue.is_empty() {
            -1
        } else {
            self.order.get(self.cursor).copied().unwrap_or(0) as i64
        };
        PlayerState {
            queue_length: self.queue.len(),
            queue_version: self.queue_version,
            index,
            cursor: self.cursor as i64,
            repeat: self.repeat,
            shuffle: self.shuffle,
            status: self.status,
            position_ms: self.position_ms.max(self.last_reported_ms),
            duration_ms: self.duration_ms,
            volume: self.volume,
            muted: self.muted,
            rate: self.rate,
            error: self.error.clone(),
            current: self.current_track().cloned(),
            token: self.token,
            updated_at: now_ms(),
        }
    }

    pub fn queue_snapshot(&self) -> QueueSnapshot {
        QueueSnapshot {
            ids: self.order.iter().map(|&i| self.queue[i].id.clone()).collect(),
            index: self.cursor,
            cursor: self.cursor,
            version: self.queue_version,
            took_ms: 0,
        }
    }

    pub fn take_pending_load(&mut self) -> Option<(u64, String, u64, bool)> {
        self.pending_load.take()
    }

    pub fn volume(&self) -> (f64, bool) {
        (self.volume, self.muted)
    }

    /// 启动恢复：按持久化状态重建 Actor 并装载到引擎（装载但暂停）。
    ///
    /// - 恢复成 `Paused`：不自动播放，用户点播放继续
    /// - shuffle 开启时重新洗牌（排列不落库，对齐持久化决策），当前曲保持在 cursor 位
    /// - `current_track_id` 按曲目 ID 定位（不传下标：DB 队列里被删曲目会让下标错位）
    /// - 恢复前调用方负责用 DB 队列重建 tracks（缺失曲目已静默剔除）
    pub fn hydrate(
        &mut self,
        tracks: Vec<TrackRef>,
        repeat: RepeatMode,
        shuffle: bool,
        volume: f64,
        muted: bool,
        rate: f64,
        position_ms: u64,
        current_track_id: Option<&str>,
    ) {
        self.queue = tracks;
        self.repeat = repeat;
        self.shuffle = shuffle;
        self.volume = volume.clamp(0.0, 1.0);
        self.muted = muted;
        self.rate = rate.clamp(0.5, 2.0);
        self.position_ms = position_ms;
        self.last_reported_ms = position_ms;
        self.rebuild_order(None);
        let target = current_track_id
            .and_then(|id| self.queue.iter().position(|t| t.id == id))
            .unwrap_or(0);
        if let Some(pos) = self.order.iter().position(|&i| i == target) {
            self.cursor = pos;
        }
        self.bump_queue_version();
        if self.queue.is_empty() {
            self.status = PlaybackStatus::Idle;
        } else {
            // 装载到引擎但暂停（autoplay=false → status=Paused）
            self.instruct_load(false, position_ms);
        }
    }

    /// 队列原序 ids（持久化用，非播放顺序 —— 随机排列不落库）。
    pub fn queue_ids(&self) -> Vec<String> {
        self.queue.iter().map(|t| t.id.clone()).collect()
    }

    /* ---------------- 内部工具 ---------------- */    fn current_track(&self) -> Option<&TrackRef> {
        self.order
            .get(self.cursor)
            .and_then(|&i| self.queue.get(i))
    }

    fn bump_queue_version(&mut self) {
        self.queue_version += 1;
    }

    fn next_token(&mut self) -> u64 {
        self.token += 1;
        self.token
    }

    /// cursor 沿 order 前进一步（repeat=all 环绕）。返回是否成功。
    fn advance(&mut self) -> bool {
        let len = self.order.len();
        if len == 0 {
            return false;
        }
        if self.cursor + 1 < len {
            self.cursor += 1;
            true
        } else {
            match self.repeat {
                RepeatMode::All => {
                    self.cursor = 0;
                    true
                }
                _ => false,
            }
        }
    }

    /// cursor 后退一步（repeat=all 环绕）。
    fn rewind(&mut self) -> bool {
        let len = self.order.len();
        if len == 0 {
            return false;
        }
        if self.cursor > 0 {
            self.cursor -= 1;
            true
        } else {
            match self.repeat {
                RepeatMode::All => {
                    self.cursor = len - 1;
                    true
                }
                _ => false,
            }
        }
    }

    /// 指挥装载当前曲目。autoplay 决定装载后是否起播。
    fn instruct_load(&mut self, autoplay: bool, start_ms: u64) {
        let track = self.current_track().cloned();
        if let Some(track) = track {
            let token = self.next_token();
            self.pending_load = Some((token, track.path.clone(), start_ms, autoplay));
            self.status = if autoplay { PlaybackStatus::Loading } else { PlaybackStatus::Paused };
            self.position_ms = start_ms;
            self.duration_ms = track.duration_ms.map(|d| d.max(0) as u64);
            self.error = None;
        } else {
            self.status = PlaybackStatus::Idle;
        }
    }

    /* ---------------- 命令归约 ---------------- */

    pub fn reduce(&mut self, cmd: PlayerCommand) {
        match cmd {
            PlayerCommand::TogglePause => match self.status {
                PlaybackStatus::Playing => {
                    self.status = PlaybackStatus::Paused;
                }
                PlaybackStatus::Paused => {
                    self.status = PlaybackStatus::Playing;
                }
                PlaybackStatus::Idle | PlaybackStatus::Error => {
                    if !self.queue.is_empty() {
                        self.instruct_load(true, 0);
                    }
                }
                PlaybackStatus::Loading => {}
            },

            PlayerCommand::Next => {
                if self.advance() {
                    self.instruct_load(true, 0);
                } else {
                    // 队列尽头：停住
                    self.status = PlaybackStatus::Idle;
                    self.position_ms = 0;
                }
            }

            PlayerCommand::Prev => {
                // 播放超过 3 秒 → 回到本曲开头（通用播放器惯例）
                if self.position_ms > 3_000 && self.current_track().is_some() {
                    self.instruct_load(true, 0);
                } else if self.rewind() {
                    self.instruct_load(true, 0);
                } else {
                    self.instruct_load(true, 0);
                }
            }

            PlayerCommand::Seek(ms) => {
                self.position_ms = ms;
                self.last_reported_ms = ms;
                // Seek 只作用于当前装载；引擎适配层直接收到独立 Seek 指令
            }

            PlayerCommand::SetVolume { volume, muted } => {
                self.volume = volume.clamp(0.0, 1.0);
                self.muted = muted;
            }

            PlayerCommand::SetRepeat(mode) => self.repeat = mode,

            PlayerCommand::SetRate(rate) => {
                // 0.5–2.0，0.1 网格外 clamp；引擎侧变速 P2 后期接 DSP
                self.rate = rate.clamp(0.5, 2.0);
            }

            PlayerCommand::ToggleShuffle => {
                self.shuffle = !self.shuffle;
                let current_queue_index = self.order.get(self.cursor).copied();
                self.rebuild_order(current_queue_index);
                self.bump_queue_version();
            }

            PlayerCommand::Queue(op) => match op {
                QueueOp::ReplaceAll { tracks, start } => {
                    let start = start.min(tracks.len().saturating_sub(1));
                    self.queue = tracks;
                    self.rebuild_order(None);
                    self.cursor = start.min(self.order.len().saturating_sub(1)).max(0);
                    self.cursor = start;
                    self.bump_queue_version();
                    self.instruct_load(true, 0);
                }
                QueueOp::Append { tracks } => {
                    let first_new = self.queue.len();
                    self.queue.extend(tracks);
                    // order 重新构建（保持当前播放位置）
                    let current_queue_index = self.order.get(self.cursor).copied();
                    self.rebuild_order(current_queue_index);
                    self.bump_queue_version();
                    // 空队列追加后自动开始播放
                    if self.status == PlaybackStatus::Idle && !self.order.is_empty() {
                        self.cursor = self
                            .order
                            .iter()
                            .position(|&i| i == first_new)
                            .unwrap_or(0);
                        self.instruct_load(true, 0);
                    }
                }
                QueueOp::PlayNext { tracks } => {
                    let insert_at = self.cursor + 1;
                    let first_new = self.order.len().min(insert_at);
                    for (k, t) in tracks.into_iter().enumerate() {
                        let queue_idx = self.queue.len();
                        self.queue.push(t);
                        self.order.insert(first_new + k, queue_idx);
                    }
                    self.bump_queue_version();
                }
                QueueOp::Remove { indices } => {
                    let mut to_remove: Vec<usize> = indices;
                    to_remove.sort_unstable();
                    to_remove.dedup();
                    let removed_before = to_remove.iter().filter(|&&i| {
                        self.order
                            .get(self.cursor)
                            .map(|&c| i < c)
                            .unwrap_or(false)
                    });
                    let n_before = removed_before.count();
                    let current_removed = to_remove
                        .iter()
                        .any(|&i| self.order.get(self.cursor) == Some(&i));
                    // 从 queue 里删（倒序防错位）
                    for &i in to_remove.iter().rev() {
                        if i < self.queue.len() {
                            self.queue.remove(i);
                        }
                    }
                    // 重建 order（删除后的下标已经移位）
                    let was_playing_at = if current_removed {
                        // 删除了当前曲目：跳到下一个（同 cursor 位置）
                        Some(self.cursor.saturating_sub(n_before))
                    } else {
                        None
                    };
                    self.rebuild_order(None);
                    if let Some(pos) = was_playing_at {
                        self.cursor = pos.min(self.order.len().saturating_sub(1));
                    } else {
                        // 未删除当前曲目：cursor 前移补偿
                        self.cursor = self.cursor.saturating_sub(n_before);
                    }
                    self.bump_queue_version();
                    if current_removed {
                        if self.order.is_empty() {
                            self.status = PlaybackStatus::Idle;
                            self.position_ms = 0;
                        } else {
                            self.instruct_load(true, 0);
                        }
                    }
                }
            },

            PlayerCommand::Loaded { token, duration_ms } => {
                if token == self.token {
                    self.duration_ms = Some(duration_ms);
                    if self.status == PlaybackStatus::Loading {
                        self.status = PlaybackStatus::Playing;
                    }
                }
            }

            PlayerCommand::Position { position_ms } => {
                self.last_reported_ms = position_ms;
                self.position_ms = position_ms;
            }

            PlayerCommand::Ended { token } => {
                if token != self.token {
                    return; // 过期回报丢弃
                }
                // repeat=one：重播本曲
                if self.repeat == RepeatMode::One && self.current_track().is_some() {
                    self.instruct_load(true, 0);
                    return;
                }
                if self.advance() {
                    self.instruct_load(true, 0);
                } else {
                    self.status = PlaybackStatus::Idle;
                    self.position_ms = 0;
                    self.last_reported_ms = 0;
                }
            }

            PlayerCommand::Failed { message } => {
                self.error = Some(message);
                self.status = PlaybackStatus::Error;
            }
        }
    }

    /// 重建播放顺序。`keep` 指定要保持在 cursor 位置的原 queue 下标。
    fn rebuild_order(&mut self, keep: Option<usize>) {
        let n = self.queue.len();
        let mut order: Vec<usize> = (0..n).collect();
        if self.shuffle && n > 1 {
            // 洗牌：deterministic 简化版 —— v1 用时间熵（UI 不需要可复现性）
            let seed = now_ms();
            // xorshift
            let mut x = seed | 1;
            for i in (1..n).rev() {
                x ^= x << 13;
                x ^= x >> 7;
                x ^= x << 17;
                let j = (x % (i as u64 + 1)) as usize;
                order.swap(i, j);
            }
            // 让 keep 的曲目排到当前位置附近（自然体验：shuffle 不打断当前歌）
            if let Some(k) = keep {
                if let Some(pos) = order.iter().position(|&i| i == k) {
                    let target = order.len().min(1); // 洗牌后当前歌放最前
                    let _ = target;
                    let pos = pos;
                    let swap_to = 0.min(order.len() - 1);
                    if pos != swap_to {
                        order.swap(pos, swap_to);
                    }
                }
            }
        }
        self.order = order;
        // cursor 指向 keep 的位置
        if let Some(k) = keep {
            if let Some(pos) = self.order.iter().position(|&i| i == k) {
                self.cursor = pos;
                return;
            }
        }
        if self.cursor >= self.order.len() {
            self.cursor = 0;
        }
    }
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

#[cfg(test)]
mod tests {
    use super::*;

    fn track(id: &str) -> TrackRef {
        TrackRef {
            id: id.into(),
            title: format!("T{id}"),
            artist: None,
            album: None,
            duration_ms: Some(180_000),
            artwork_id: None,
            gain_db: None,
            path: format!("D:/m/{id}.flac"),
        }
    }

    #[test]
    fn replace_all_plays_first_track() {
        let mut p = PlayerActor::new();
        p.reduce(PlayerCommand::Queue(QueueOp::ReplaceAll {
            tracks: vec![track("a"), track("b"), track("c")],
            start: 0,
        }));
        let (token, path, start, autoplay) = p.take_pending_load().unwrap();
        assert_eq!(path, "D:/m/a.flac");
        assert!(autoplay);
        assert_eq!(start, 0);
        let s = p.state();
        assert_eq!(s.index, 0);
        assert_eq!(s.status, PlaybackStatus::Loading);
        assert_eq!(s.token, token);

        p.reduce(PlayerCommand::Loaded { token, duration_ms: 180_000 });
        assert_eq!(p.state().status, PlaybackStatus::Playing);
    }

    #[test]
    fn eof_advances_repeat_off_stops_at_end() {
        let mut p = PlayerActor::new();
        p.reduce(PlayerCommand::Queue(QueueOp::ReplaceAll {
            tracks: vec![track("a"), track("b")],
            start: 0,
        }));
        let (t1, ..) = p.take_pending_load().unwrap();
        p.reduce(PlayerCommand::Loaded { token: t1, duration_ms: 1 });

        p.reduce(PlayerCommand::Ended { token: t1 });
        let (_, path, ..) = p.take_pending_load().unwrap();
        assert_eq!(path, "D:/m/b.flac");

        let t2 = p.take_pending_load(); // 不存在第二次 load
        assert!(t2.is_none());
        let token_now = p.state().token;
        p.reduce(PlayerCommand::Ended { token: token_now });
        assert_eq!(p.state().status, PlaybackStatus::Idle, "repeat=off 队尾停住");
        assert!(p.take_pending_load().is_none());
    }

    #[test]
    fn repeat_one_replays() {
        let mut p = PlayerActor::new();
        p.reduce(PlayerCommand::Queue(QueueOp::ReplaceAll {
            tracks: vec![track("a"), track("b")],
            start: 0,
        }));
        let (t1, ..) = p.take_pending_load().unwrap();
        p.reduce(PlayerCommand::Loaded { token: t1, duration_ms: 1 });
        p.reduce(PlayerCommand::SetRepeat(RepeatMode::One));

        p.reduce(PlayerCommand::Ended { token: t1 });
        let (t2, path, start, autoplay) = p.take_pending_load().unwrap();
        assert_eq!(path, "D:/m/a.flac", "repeat=one 重播本曲");
        assert_eq!(start, 0);
        assert!(autoplay);
        assert_ne!(t2, t1, "token 递增");
    }

    #[test]
    fn repeat_all_wraps() {
        let mut p = PlayerActor::new();
        p.reduce(PlayerCommand::Queue(QueueOp::ReplaceAll {
            tracks: vec![track("a"), track("b")],
            start: 0,
        }));
        let (t1, ..) = p.take_pending_load().unwrap();
        p.reduce(PlayerCommand::Loaded { token: t1, duration_ms: 1 });
        p.reduce(PlayerCommand::SetRepeat(RepeatMode::All));

        p.reduce(PlayerCommand::Ended { token: t1 });
        let (_, path, ..) = p.take_pending_load().unwrap();
        assert_eq!(path, "D:/m/b.flac");

        p.reduce(PlayerCommand::Ended { token: p.state().token });
        let (_, path, ..) = p.take_pending_load().unwrap();
        assert_eq!(path, "D:/m/a.flac", "repeat=all 环绕回第一首");
    }

    #[test]
    fn prev_restarts_after_three_seconds() {
        let mut p = PlayerActor::new();
        p.reduce(PlayerCommand::Queue(QueueOp::ReplaceAll {
            tracks: vec![track("a"), track("b")],
            start: 0,
        }));
        let (t1, ..) = p.take_pending_load().unwrap();
        p.reduce(PlayerCommand::Loaded { token: t1, duration_ms: 1 });
        p.reduce(PlayerCommand::Position { position_ms: 5_000 });

        p.reduce(PlayerCommand::Prev);
        let (_, path, start, _) = p.take_pending_load().unwrap();
        assert_eq!(path, "D:/m/a.flac", "5 秒处 prev 回本曲开头");
        assert_eq!(start, 0);
    }

    #[test]
    fn shuffle_keeps_current_then_restores_order() {
        let mut p = PlayerActor::new();
        p.reduce(PlayerCommand::Queue(QueueOp::ReplaceAll {
            tracks: (0..10).map(|i| track(&format!("t{i}"))).collect(),
            start: 3,
        }));
        let _ = p.take_pending_load().unwrap();
        assert_eq!(p.state().index, 3);

        p.reduce(PlayerCommand::ToggleShuffle);
        let s = p.state();
        assert!(s.shuffle);
        // shuffle 后当前曲目仍在播放（同一首）
        assert_eq!(s.current.unwrap().id, "t3");
        // 洗牌后顺序应是原 10 首的排列
        let snap = p.queue_snapshot();
        let mut ids = snap.ids.clone();
        ids.sort();
        assert_eq!(ids.len(), 10);

        p.reduce(PlayerCommand::ToggleShuffle);
        let snap2 = p.queue_snapshot();
        assert_eq!(snap2.ids[0], "t0", "关闭 shuffle 恢复原序");
        // 当前曲目还是 t3
        assert_eq!(p.state().current.unwrap().id, "t3");
    }

    #[test]
    fn play_next_inserts_after_current() {
        let mut p = PlayerActor::new();
        p.reduce(PlayerCommand::Queue(QueueOp::ReplaceAll {
            tracks: vec![track("a"), track("b")],
            start: 0,
        }));
        let _ = p.take_pending_load().unwrap();
        p.reduce(PlayerCommand::Queue(QueueOp::PlayNext { tracks: vec![track("x")] }));

        let snap = p.queue_snapshot();
        assert_eq!(snap.ids, vec!["a", "x", "b"], "PlayNext 插到当前之后");

        p.reduce(PlayerCommand::Ended { token: p.state().token });
        let (_, path, ..) = p.take_pending_load().unwrap();
        assert_eq!(path, "D:/m/x.flac", "播完当前播插入曲");
    }

    #[test]
    fn remove_current_advances_to_next() {
        let mut p = PlayerActor::new();
        p.reduce(PlayerCommand::Queue(QueueOp::ReplaceAll {
            tracks: vec![track("a"), track("b"), track("c")],
            start: 1,
        }));
        let _ = p.take_pending_load().unwrap();

        p.reduce(PlayerCommand::Queue(QueueOp::Remove { indices: vec![1] }));
        let (_, path, ..) = p.take_pending_load().unwrap();
        assert_eq!(path, "D:/m/c.flac", "删除当前曲后播下一首（原 c）");
        assert_eq!(p.queue_snapshot().ids, vec!["a", "c"]);
    }

    #[test]
    fn stale_tokens_are_dropped() {
        let mut p = PlayerActor::new();
        p.reduce(PlayerCommand::Queue(QueueOp::ReplaceAll {
            tracks: vec![track("a")],
            start: 0,
        }));
        let (t1, ..) = p.take_pending_load().unwrap();
        p.reduce(PlayerCommand::Loaded { token: t1, duration_ms: 1 });

        // 用旧 token 报 Ended 应被丢弃
        p.reduce(PlayerCommand::Ended { token: t1 - 1 });
        assert!(p.take_pending_load().is_none(), "过期 token 不触发推进");
        assert_eq!(p.state().status, PlaybackStatus::Playing);
    }

    #[test]
    fn append_to_empty_autoplays() {
        let mut p = PlayerActor::new();
        p.reduce(PlayerCommand::Queue(QueueOp::Append { tracks: vec![track("a")] }));
        let (_, path, _, autoplay) = p.take_pending_load().unwrap();
        assert_eq!(path, "D:/m/a.flac");
        assert!(autoplay);
    }

    #[test]
    fn hydrate_restores_paused_at_position() {
        let mut p = PlayerActor::new();
        p.hydrate(
            vec![track("a"), track("b"), track("c")],
            RepeatMode::All,
            false,
            0.7,
            false,
            1.25,
            30_000,
            Some("b"),
        );
        let (token, path, start, autoplay) = p.take_pending_load().unwrap();
        assert_eq!(path, "D:/m/b.flac", "恢复到 current= b 的曲目");
        assert_eq!(start, 30_000, "从持久化位置继续");
        assert!(!autoplay, "恢复不自动播放");
        let s = p.state();
        assert_eq!(s.status, PlaybackStatus::Paused);
        assert_eq!(s.repeat, RepeatMode::All);
        assert_eq!(s.volume, 0.7);
        assert_eq!(s.rate, 1.25);
        assert_eq!(s.position_ms, 30_000);
        assert_eq!(s.token, token);
        assert_eq!(s.index, 1);
        assert_eq!(p.queue_ids(), vec!["a", "b", "c"], "queue_ids 是原序");

        // 恢复后点播放：Paused → Playing，不再触发新 load
        p.reduce(PlayerCommand::TogglePause);
        assert!(p.take_pending_load().is_none());
        assert_eq!(p.state().status, PlaybackStatus::Playing);
    }

    #[test]
    fn hydrate_shuffle_keeps_current_track() {
        let mut p = PlayerActor::new();
        p.hydrate(
            (0..8).map(|i| track(&format!("t{i}"))).collect(),
            RepeatMode::Off,
            true,
            0.85,
            false,
            1.0,
            0,
            Some("t5"),
        );
        let _ = p.take_pending_load().unwrap();
        assert_eq!(p.state().current.unwrap().id, "t5", "洗牌后当前曲仍是 t5");
        let mut ids = p.queue_snapshot().ids;
        ids.sort();
        assert_eq!(ids.len(), 8, "洗牌是排列，不丢曲");
    }

    #[test]
    fn hydrate_missing_current_falls_back_to_first() {
        let mut p = PlayerActor::new();
        p.hydrate(
            vec![track("a"), track("b")],
            RepeatMode::Off,
            false,
            0.85,
            false,
            1.0,
            0,
            Some("deleted"),
        );
        let (_, path, ..) = p.take_pending_load().unwrap();
        assert_eq!(path, "D:/m/a.flac", "当前曲已被删 → 回退第一首");
    }

    #[test]
    fn hydrate_empty_queue_stays_idle() {
        let mut p = PlayerActor::new();
        p.hydrate(vec![], RepeatMode::Off, false, 0.85, false, 1.0, 0, None);
        assert!(p.take_pending_load().is_none());
        assert_eq!(p.state().status, PlaybackStatus::Idle);
    }
}
