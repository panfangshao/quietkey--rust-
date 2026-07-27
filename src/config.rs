//! 配置的持久化与目标匹配规则。
//!
//! 关键设计：**不存 HWND**。窗口句柄在目标程序重启后就失效了，
//! 所以存的是一组匹配规则（进程名 / 标题子串 / 窗口类名），
//! 每次触发热键时重新解析成当前的 HWND。

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

pub const APP_NAME: &str = "quietkey";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    /// global-hotkey 的组合键描述，例如 "Alt+KeyQ"、"Control+Shift+KeyP"。
    pub hotkey: String,
    /// 视频窗口的匹配规则。
    pub target: TargetSpec,
    /// 允许启用/禁用策略链中的每一层，便于排查问题。
    pub strategies: StrategyPrefs,
    /// 兜底方案（抢焦点）在切走后等待多久再发按键，单位毫秒。
    pub focus_settle_ms: u64,
    /// 启动时直接缩到托盘，不弹主窗口。
    pub start_hidden: bool,
    /// 第 1 层挑会话时，优先跟随**你最近动过的那个播放器**（见 `strategy::session_watch`）。
    ///
    /// 多个浏览器同时在播视频时才有区别。`target.exe` 填了进程名的话仍以配置为准，
    /// 这一项只在没显式指定目标时接管。
    pub follow_recent_session: bool,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            hotkey: "Alt+KeyQ".to_string(),
            target: TargetSpec::default(),
            strategies: StrategyPrefs::default(),
            focus_settle_ms: 30,
            start_hidden: false,
            // 默认开：多播放器场景下"跟着人走"才符合直觉，
            // 单播放器场景下它不改变任何行为（只有一个会话可选）。
            follow_recent_session: true,
        }
    }
}

/// 目标窗口的匹配规则。多个字段之间是 **与** 的关系，为空的字段不参与匹配。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct TargetSpec {
    /// 进程可执行文件名，不区分大小写，例如 "chrome.exe"。
    pub exe: Option<String>,
    /// 窗口标题包含该子串，不区分大小写。
    pub title_contains: Option<String>,
    /// 窗口类名，精确匹配。浏览器是 "Chrome_WidgetWin_1"。
    pub class_name: Option<String>,
}

impl TargetSpec {
    pub fn is_empty(&self) -> bool {
        self.exe.as_deref().unwrap_or("").is_empty()
            && self.title_contains.as_deref().unwrap_or("").is_empty()
            && self.class_name.as_deref().unwrap_or("").is_empty()
    }

    /// 人类可读的摘要，用于 UI 展示。
    pub fn describe(&self) -> String {
        let mut parts = Vec::new();
        if let Some(exe) = self.exe.as_deref().filter(|s| !s.is_empty()) {
            parts.push(format!("进程={exe}"));
        }
        if let Some(t) = self.title_contains.as_deref().filter(|s| !s.is_empty()) {
            parts.push(format!("标题包含=\"{t}\""));
        }
        if let Some(c) = self.class_name.as_deref().filter(|s| !s.is_empty()) {
            parts.push(format!("类名={c}"));
        }
        if parts.is_empty() {
            "（未设置）".to_string()
        } else {
            parts.join("  ")
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct StrategyPrefs {
    /// 第 1 层：Windows 媒体会话（GSMTC）。浏览器网页视频的主力，零感知。
    pub gsmtc: bool,
    /// 第 2 层：WM_APPCOMMAND 定向投递。VLC / PotPlayer 等，零感知。
    pub appcommand: bool,
    /// 第 3 层：PostMessage 键盘消息。原生 Win32 播放器。
    pub postmessage: bool,
    /// 第 4 层：抢焦点 + SendInput + 还原。**会有短暂闪烁**，兜底用。
    pub focus_sendinput: bool,
}

impl Default for StrategyPrefs {
    fn default() -> Self {
        Self {
            gsmtc: true,
            appcommand: true,
            postmessage: true,
            // 兜底层默认关闭：它是唯一会打断你打字的方案，
            // 让用户在确认前三层都不管用之后再主动开启。
            focus_sendinput: false,
        }
    }
}

pub fn config_path() -> PathBuf {
    let base = std::env::var_os("APPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir);
    base.join(APP_NAME).join("config.toml")
}

impl Config {
    pub fn load() -> Self {
        let path = config_path();
        match std::fs::read_to_string(&path) {
            Ok(text) => toml::from_str(&text).unwrap_or_else(|e| {
                log::warn!("配置解析失败，使用默认值: {e}");
                Config::default()
            }),
            Err(_) => Config::default(),
        }
    }

    pub fn save(&self) -> anyhow::Result<()> {
        let path = config_path();
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        std::fs::write(&path, toml::to_string_pretty(self)?)?;
        Ok(())
    }
}
