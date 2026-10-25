//! PlayerActor：播放状态机唯一真相。
//!
//! 纯逻辑（不碰线程/引擎）：命令进 → 状态变 + 零或一跳出 → 音频出口指令出。
//! 引擎适配层（src-tauri）负责把跳出的指令翻译成 aiya-audio 的 EngineCommand，
//! 并把 EngineEvent 反喂成 PlayerCommand。
//!
//! 顺序模型（对齐 contracts）：`queue` 是原序，`order` 是播放顺序
//! （shuffle 开启时打乱）。`cursor` 在 order 里走，`index` 是 order[cursor]。

use crate::state::{
    AudioError, AudioSource, PlaybackStatus, PlayerCommand, PlayerState, QueueOp, QueueSnapshot,
    RepeatMode, TrackRef,
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
    error: Option<AudioError>,
    queue_version: u64,
    token: u64,
    /// 最近一次需要发出的引擎指令（Actor 不直接驱动线程）
    pending_load: Option<(u64, String, u64, bool)>, // (token, path, startMs, autoplay)
    /// 需要下发 EngineCommand::Stop（队尾停住/删掉当前曲且队列空）。
    /// 此前只有 pending_load，这两种迁移对引擎完全静默：UI 显示已停、
    /// 声音继续放到本曲结束。
    pending_stop: bool,
    /// Actor 自己维护的单调位置估计（无引擎回报时给 UI）
    last_reported_ms: u64,
    /// 当前曲目音源类别（docs/22 G3）。instruct_load 时按路径前缀判定：
    /// http(s) = Stream（FM 推流），否则 File。
    source: AudioSource,
    /// ReplayGain 归一化开关（设置项 normalizeVolume）。放在 Actor 里而不是
    /// 宿主里：音量是 Actor 的状态，gain_db 也在 Actor 手里（current），
    /// 分两处算会重复且容易不同步（旧 UI 的 setNormalization 已废弃为 no-op）。
    normalize: bool,
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
            pending_stop: false,
            last_reported_ms: 0,
            source: AudioSource::File,
            normalize: false,
        }
    }

    /* ---------------- 对外查询 ---------------- */

    pub fn state(&self) -> PlayerState {
        // 空队列 → index/cursor 都是 None（不用 -1 哨兵，见 PlayerState 注释）。
        // 队列非空时 order 与 queue 同步重建，order[cursor] 必然存在；
        // 仍用 get() 防御而不是 unwrap，让"不可能"只降级成 null 而不是 panic。
        let has_queue = !self.queue.is_empty();
        let index = if has_queue {
            self.order.get(self.cursor).copied()
        } else {
            None
        };
        let cursor = if has_queue { Some(self.cursor) } else { None };
        PlayerState {
            queue_length: self.queue.len(),
            queue_version: self.queue_version,
            index,
            cursor,
            repeat: self.repeat,
            shuffle: self.shuffle,
            status: self.status,
            source: self.source,
            // 不再 max(last_reported_ms)：那是"上一曲末尾"的残留值，
            // 切曲瞬间会把旧进度挂到新曲上（时长对不上，进度条跳回）。
            position_ms: self.position_ms,
            duration_ms: self.duration_ms,
            volume: self.volume,
            muted: self.muted,
            rate: self.rate,
            error: self.error.clone(),
            current: self.current_track().cloned(),
            token: self.token,
            updated_at_ms: now_ms(),
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

    /// 取走"需要停引擎"的标记（队尾停住 / 队列清空）。
    pub fn take_pending_stop(&mut self) -> bool {
        std::mem::take(&mut self.pending_stop)
    }

    pub fn volume(&self) -> (f64, bool) {
        (self.volume, self.muted)
    }

    /// 归一化开关（宿主从设置里同步；换曲不用重设，gain_db 从 current 现取）
    pub fn set_normalize(&mut self, on: bool) {
        self.normalize = on;
    }

    /// ReplayGain 线性增益倍数。关闭 / 曲目无 `gain_db` 时为 1.0。
    ///
    /// 限幅 ±12 dB：beets 的 `rg_track_gain` 来自标签，坏标签能写出 −40 dB
    /// 这种值，直接乘会把声音"搞没"。上限 +12 dB 实际还会被引擎的音量
    /// 上限（1.0）夹住——我们不承诺"响度拉平"，只承诺"按标签补偿"。
    pub fn norm_factor(&self) -> f32 {
        if !self.normalize {
            return 1.0;
        }
        let db = self
            .current_track()
            .and_then(|t| t.gain_db)
            .unwrap_or(0.0)
            .clamp(-12.0, 12.0);
        10f32.powf((db / 20.0) as f32)
    }

    /// 实际送给引擎的音量：静音为 0，否则用户音量 × 归一化倍数，夹到 [0,1]。
    /// **唯一**计算点——宿主三处下发（装载/拖动/静音）都走这里，避免各算一遍。
    pub fn engine_volume(&self) -> f32 {
        if self.muted {
            return 0.0;
        }
        (self.volume as f32 * self.norm_factor()).clamp(0.0, 1.0)
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
            self.go_idle();
        } else {
            // 装载到引擎但暂停（autoplay=false → status=Paused）
            self.instruct_load(false, position_ms);
        }
    }

    /// 队列原序 ids（持久化用，非播放顺序 —— 随机排列不落库）。
    pub fn queue_ids(&self) -> Vec<String> {
        self.queue.iter().map(|t| t.id.clone()).collect()
    }

    /* ---------------- 内部工具 ---------------- */

    fn current_track(&self) -> Option<&TrackRef> {
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
            // 音源类别在产生处判定（docs/22 G3）：http(s) 路径 = 网络流。
            // 引擎侧 open_source 用同一前缀规则开 HLS，两处必须一致。
            self.source = if track.path.starts_with("http://") || track.path.starts_with("https://")
            {
                AudioSource::Stream
            } else {
                AudioSource::File
            };
            self.status = if autoplay { PlaybackStatus::Loading } else { PlaybackStatus::Paused };
            self.position_ms = start_ms;
            self.last_reported_ms = start_ms; // 换曲必须重置，否则快照带上上一曲的残留进度
            self.pending_stop = false; // 新的装载取代旧的停止意图
            self.duration_ms = track.duration_ms.map(|d| d.max(0) as u64);
            self.error = None;
        } else {
            self.go_idle();
        }
    }

    /// 回到"没有当前曲目"：状态 Idle，**音源类别也复位**。
    ///
    /// 不复位就会留下一个假象：停掉一路 FM 之后队列空了，界面还按
    /// `source = stream` 渲染（进度条只读、时长位写"直播中"），
    /// 和一个已经不存在的主播绑在一起。
    fn go_idle(&mut self) {
        self.status = PlaybackStatus::Idle;
        self.source = AudioSource::File;
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
                // 暂停中点"下一首"应保持暂停（主流播放器行为）；只有正在播才起播
                let autoplay = self.status == PlaybackStatus::Playing;
                if self.advance() {
                    self.instruct_load(autoplay, 0);
                } else {
                    // 队列尽头：停住 —— 必须下发 Stop，否则引擎继续把这首放完
                    self.go_idle();
                    self.position_ms = 0;
                    self.last_reported_ms = 0;
                    self.pending_stop = true;
                }
            }

            PlayerCommand::Prev => {
                let autoplay = self.status == PlaybackStatus::Playing;
                // 播放超过 3 秒 → 回到本曲开头（通用播放器惯例）
                if self.position_ms > 3_000 && self.current_track().is_some() {
                    self.instruct_load(autoplay, 0);
                } else {
                    // 到队首且 repeat=off 时 rewind 失败，仍按"重播本曲"处理
                    let _ = self.rewind();
                    self.instruct_load(autoplay, 0);
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
                    self.queue = tracks;
                    let n = self.queue.len();
                    if n == 0 {
                        self.go_idle();
                        self.position_ms = 0;
                        self.last_reported_ms = 0;
                        self.pending_stop = true;
                        self.bump_queue_version();
                        return;
                    }
                    let start = start.min(n - 1);
                    self.rebuild_order(None);
                    // start 是"队列原序"下标，必须换算成 order 里的位置 ——
                    // 洗牌后 cursor 直接用 start 会播到另一首
                    self.cursor = self
                        .order
                        .iter()
                        .position(|&i| i == start)
                        .unwrap_or(0);
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
                    let was_playing = self.status == PlaybackStatus::Playing;
                    // indices 是**播放顺序里的位置**（UI 拿到的是 queue_snapshot().ids
                    // 的下标）。此前当成 queue 下标用：洗牌时删错歌，且
                    // rebuild_order(None) 会把整队重洗 —— 删一首就打乱全部顺序。
                    let mut positions: Vec<usize> = indices
                        .into_iter()
                        .filter(|&p| p < self.order.len())
                        .collect();
                    positions.sort_unstable();
                    positions.dedup();
                    if positions.is_empty() {
                        return;
                    }
                    let cur_pos = self.cursor;
                    let cur_queue_idx = self.order.get(cur_pos).copied();
                    let removed_before_cursor =
                        positions.iter().filter(|&&p| p < cur_pos).count();
                    let current_removed = cur_queue_idx
                        .map(|c| positions.iter().any(|&p| self.order[p] == c))
                        .unwrap_or(false);

                    // 位置 → queue 下标
                    let mut queue_idx: Vec<usize> =
                        positions.iter().map(|&p| self.order[p]).collect();
                    queue_idx.sort_unstable();
                    queue_idx.dedup();
                    for &qi in queue_idx.iter().rev() {
                        if qi < self.queue.len() {
                            self.queue.remove(qi);
                        }
                    }
                    // 重建 order：过滤被删位置、剩余下标按删除数量平移，
                    // 保持相对顺序（不重洗）
                    let mut new_order = Vec::with_capacity(self.order.len() - positions.len());
                    for (pos, &qi) in self.order.iter().enumerate() {
                        if positions.binary_search(&pos).is_ok() {
                            continue;
                        }
                        let shift = queue_idx.iter().filter(|&&d| d < qi).count();
                        new_order.push(qi - shift);
                    }
                    self.order = new_order;
                    self.bump_queue_version();

                    if current_removed {
                        // cursor 原地指向"下一首"；删的是末尾则回退一格
                        self.cursor = cur_pos.min(self.order.len().saturating_sub(1));
                        if self.order.is_empty() {
                            self.go_idle();
                            self.position_ms = 0;
                            self.last_reported_ms = 0;
                            self.pending_stop = true;
                        } else {
                            self.instruct_load(was_playing, 0);
                        }
                    } else {
                        self.cursor = cur_pos.saturating_sub(removed_before_cursor);
                    }
                }
                QueueOp::Move { from, to } => {
                    // 与 Remove 同一坐标系：order 里的位置
                    if from >= self.order.len() || to >= self.order.len() || from == to {
                        return;
                    }
                    let cur_queue_idx = self.order.get(self.cursor).copied();
                    let moved = self.order.remove(from);
                    self.order.insert(to, moved);
                    // cursor 跟着"当前这首"走
                    if let Some(c) = cur_queue_idx {
                        if let Some(pos) = self.order.iter().position(|&i| i == c) {
                            self.cursor = pos;
                        }
                    }
                    self.bump_queue_version();
                }
            },

            PlayerCommand::Loaded { token, duration_ms } => {
                if token == self.token {
                    // 0 = 直播流没有总时长（docs/22 G3）：对 UI 呈现 None
                    // 而不是 Some(0)——进度条按 source=stream 显示"直播中"
                    self.duration_ms = if duration_ms == 0 { None } else { Some(duration_ms) };
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
                    self.go_idle();
                    self.position_ms = 0;
                    self.last_reported_ms = 0;
                    self.pending_stop = true; // 队尾：引擎必须真的停
                }
            }

            PlayerCommand::Failed { error } => {
                self.error = Some(error);
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
            // 让 keep 的曲目排到最前（shuffle 不打断当前歌）：
            // 先洗牌再把当前曲换到 0 位，cursor 随后由下方统一指向它
            if let Some(k) = keep {
                if let Some(pos) = order.iter().position(|&i| i == k) {
                    if pos != 0 {
                        order.swap(pos, 0);
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
            audio: None,
            path: format!("D:/m/{id}.flac"),
        }
    }

    #[test]
    fn empty_queue_state_uses_none_not_minus_one() {
        let p = PlayerActor::new();
        let s = p.state();
        assert_eq!(s.index, None, "空队列 index=null（不用 -1 哨兵）");
        assert_eq!(s.cursor, None, "空队列 cursor=null");
        assert!(s.current.is_none());
        assert_eq!(s.queue_length, 0);
    }

    #[test]
    fn stream_source_resets_when_queue_is_cleared() {
        // 队列里放一路 FM（http 路径）→ source=Stream
        let mut fm = track("fm");
        fm.path = "http://fm.test/live/stream.m3u8".into();
        fm.duration_ms = None;
        let mut p = PlayerActor::new();
        p.reduce(PlayerCommand::Queue(QueueOp::ReplaceAll { tracks: vec![fm], start: 0 }));
        assert_eq!(p.state().source, AudioSource::Stream, "http 路径必须判成流");

        // 清空队列 → 不能继续声称自己是 stream（否则 UI 一直"直播中"）
        p.reduce(PlayerCommand::Queue(QueueOp::ReplaceAll { tracks: vec![], start: 0 }));
        let s = p.state();
        assert_eq!(s.status, PlaybackStatus::Idle);
        assert_eq!(s.source, AudioSource::File, "队列空了，source 必须复位");
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
        assert_eq!(s.index, Some(0));
        assert_eq!(s.status, PlaybackStatus::Loading);
        assert_eq!(s.token, token);

        p.reduce(PlayerCommand::Loaded { token, duration_ms: 180_000 });
        assert_eq!(p.state().status, PlaybackStatus::Playing);
    }

    /// 直播的 `Loaded.duration_ms = 0` 必须呈现成 `duration_ms = null`：
    /// UI 靠"有时长才画进度条"决定进度条形态，`Some(0)` 会画出一条永远 0% 的条。
    #[test]
    fn loaded_zero_duration_becomes_none_for_streams() {
        let mut fm = track("fm");
        fm.path = "http://fm.test/live/stream.m3u8".into();
        fm.duration_ms = None;
        let mut p = PlayerActor::new();
        p.reduce(PlayerCommand::Queue(QueueOp::ReplaceAll { tracks: vec![fm], start: 0 }));
        let (token, _, _, _) = p.take_pending_load().unwrap();
        p.reduce(PlayerCommand::Loaded { token, duration_ms: 0 });
        let s = p.state();
        assert_eq!(s.duration_ms, None, "直播无总时长 → null");
        assert_eq!(s.source, AudioSource::Stream);
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
        assert_eq!(p.state().index, Some(3));

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
        assert_eq!(s.index, Some(1));
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

    /* ---------------- ReplayGain 归一化 ---------------- */

    /// 装一首带 gain_db 的曲目并让它成为 current
    fn actor_with_gain(gain_db: Option<f64>) -> PlayerActor {
        let mut p = PlayerActor::new();
        let mut t = track("g");
        t.gain_db = gain_db;
        p.reduce(PlayerCommand::Queue(QueueOp::ReplaceAll { tracks: vec![t], start: 0 }));
        p.take_pending_load();
        p
    }

    #[test]
    fn normalize_off_keeps_volume() {
        let mut p = actor_with_gain(Some(-6.0));
        p.set_normalize(false);
        assert!((p.norm_factor() - 1.0).abs() < 1e-6);
        assert!((p.engine_volume() - p.volume().0 as f32).abs() < 1e-6);
    }

    #[test]
    fn normalize_applies_track_gain() {
        let mut p = actor_with_gain(Some(-6.0));
        p.set_normalize(true);
        // -6 dB ≈ 0.501
        assert!((p.norm_factor() - 0.5011872).abs() < 1e-4, "{}", p.norm_factor());
    }

    #[test]
    fn normalize_without_gain_is_unity() {
        let mut p = actor_with_gain(None);
        p.set_normalize(true);
        assert!((p.norm_factor() - 1.0).abs() < 1e-6);
    }

    #[test]
    fn normalize_clamps_absurd_tags() {
        // 坏标签 -60 dB 不能把声音搞没；限幅到 -12 dB
        let mut p = actor_with_gain(Some(-60.0));
        p.set_normalize(true);
        assert!((p.norm_factor() - 0.25118864).abs() < 1e-4, "{}", p.norm_factor());
    }

    #[test]
    fn engine_volume_clamps_to_unity_and_mutes() {
        let mut p = actor_with_gain(Some(9.0));
        p.set_normalize(true);
        p.reduce(PlayerCommand::SetVolume { volume: 1.0, muted: false });
        assert_eq!(p.engine_volume(), 1.0, "增益不能把音量推过 1.0");
        p.reduce(PlayerCommand::SetVolume { volume: 1.0, muted: true });
        assert_eq!(p.engine_volume(), 0.0, "静音恒为 0");
    }
}
