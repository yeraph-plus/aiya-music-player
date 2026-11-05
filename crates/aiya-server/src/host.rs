// ServerState 的宿主环境实现：beets/enrichment 编排（host-core）在 server 壳
// 的装配面——事件走总线，刷新走本 crate 的扫描管线，登记表自带。
// 与桌面壳 TauriBeetsHost 同构：包装 Arc<ServerState>，长任务线程独立持有。

use std::path::PathBuf;
use std::sync::atomic::Ordering;
use std::sync::Arc;

use aiya_host_core::BeetsHost;

use crate::state::{db_lock, settings_lock, ServerState};

pub struct ServerBeetsHost {
    st: Arc<ServerState>,
}

pub fn beets_host(st: &Arc<ServerState>) -> Arc<dyn BeetsHost> {
    Arc::new(ServerBeetsHost { st: Arc::clone(st) })
}

impl BeetsHost for ServerBeetsHost {
    fn emit(&self, channel: &'static str, payload: serde_json::Value) {
        self.st.events.emit(channel, &payload);
    }
    fn data_root(&self) -> PathBuf {
        self.st
            .db_path
            .parent()
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("."))
    }
    fn library_roots(&self) -> Option<Vec<String>> {
        db_lock(&self.st)
            .send(|conn| Ok(crate::state::read_roots(conn)))
            .ok()
    }
    fn online_files(&self) -> Option<Vec<aiya_data::repo::FileFact>> {
        db_lock(&self.st)
            .send(|conn| aiya_data::repo::online_files(conn))
            .ok()
    }
    fn track_ids_by_paths(
        &self,
        paths: &[String],
    ) -> Option<std::collections::HashMap<String, String>> {
        let paths = paths.to_vec();
        db_lock(&self.st)
            .send(move |conn| aiya_data::repo::track_ids_by_paths(conn, &paths))
            .ok()
    }
    fn setting_str(&self, key: &str) -> Option<String> {
        settings_lock(&self.st)
            .get(key)
            .and_then(serde_json::Value::as_str)
            .map(str::to_string)
    }
    fn install_registry(&self) -> &aiya_host_core::InstallRegistry {
        &self.st.beets_install
    }
    fn try_set_installing(&self) -> bool {
        self.st
            .beets_install
            .installing
            .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
            .is_ok()
    }
    fn enrich_store(&self) -> Result<std::sync::Arc<aiya_beets_store::Store>, String> {
        self.st.enrich_store()
    }
    fn kick_library_refresh(&self) {
        crate::scan::spawn_scan(&self.st, "action");
    }
}
