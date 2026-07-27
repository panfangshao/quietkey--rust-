//! UI 线程与热键工作线程之间共享的状态。

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;

use crate::config::Config;
use crate::strategy::ActionReport;

#[derive(Default)]
pub struct Shared {
    pub cfg: Mutex<Config>,
    /// 最近一次热键触发的完整报告，UI 用它显示"命中了哪一层"。
    pub last: Mutex<Option<ActionReport>>,
    pub triggers: AtomicU64,
}

impl Shared {
    pub fn new(cfg: Config) -> Self {
        Self {
            cfg: Mutex::new(cfg),
            last: Mutex::new(None),
            triggers: AtomicU64::new(0),
        }
    }

    pub fn snapshot_cfg(&self) -> Config {
        self.cfg.lock().unwrap().clone()
    }

    pub fn record(&self, report: ActionReport) {
        self.triggers.fetch_add(1, Ordering::Relaxed);
        *self.last.lock().unwrap() = Some(report);
    }

    pub fn trigger_count(&self) -> u64 {
        self.triggers.load(Ordering::Relaxed)
    }
}
