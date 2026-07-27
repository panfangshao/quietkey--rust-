//! 播放/暂停的四层策略链，按「对用户的打扰程度」从小到大排列。
//!
//! 1. [`gsmtc`]    Windows 媒体会话 —— 浏览器网页视频主力，完全无感
//! 2. [`messages`] WM_APPCOMMAND 定向投递 —— VLC / PotPlayer 等，完全无感
//! 3. [`messages`] PostMessage 键盘消息 —— 原生 Win32 播放器，完全无感
//! 4. [`fallback`] 抢焦点 + SendInput + 还原 —— **会闪一下**，最后的兜底
//!
//! 逐层尝试，第一个成功的即返回，并把每层的结果记录下来给 UI 显示，
//! 让用户一眼看出当前命中的是哪一层、有没有掉到会闪烁的兜底路径。

pub mod fallback;
pub mod gsmtc;
pub mod messages;
pub mod recency;

use std::time::{Duration, Instant};

use crate::config::{Config, TargetSpec};
use crate::win::{resolve_target, ResolvedTarget};

#[derive(Debug, Clone)]
pub enum Outcome {
    /// 这一层成功接管了本次操作。
    Applied(String),
    /// 这一层不适用于当前目标（不是失败，继续往下走）。
    NotApplicable(String),
    /// 这一层适用但执行失败。
    Failed(String),
}

impl Outcome {
    fn label(&self) -> &'static str {
        match self {
            Outcome::Applied(_) => "命中",
            Outcome::NotApplicable(_) => "跳过",
            Outcome::Failed(_) => "失败",
        }
    }

    fn detail(&self) -> &str {
        match self {
            Outcome::Applied(s) | Outcome::NotApplicable(s) | Outcome::Failed(s) => s,
        }
    }
}

/// 单次热键触发的完整报告。
#[derive(Debug, Clone)]
pub struct ActionReport {
    pub ok: bool,
    /// 最终命中的层名；没命中则为 None。
    pub winner: Option<&'static str>,
    pub detail: String,
    pub elapsed: Duration,
    /// 每一层的尝试记录，格式为 (层名, 结果标签, 说明)。
    pub trace: Vec<(&'static str, &'static str, String)>,
    /// 命中的这一层是否会打断用户（即是否掉到了兜底层）。
    pub intrusive: bool,
}

impl ActionReport {
    pub fn summary(&self) -> String {
        match self.winner {
            Some(name) => format!("{name} · {} · {:.1}ms", self.detail, self.elapsed.as_secs_f64() * 1000.0),
            None => format!("未生效 · {}", self.detail),
        }
    }
}

/// 执行一次「播放/暂停」切换，逐层降级。
pub fn toggle_play_pause(cfg: &Config) -> ActionReport {
    let started = Instant::now();
    let mut trace: Vec<(&'static str, &'static str, String)> = Vec::new();

    let target = resolve_target(&cfg.target);
    if let Some(t) = &target {
        // 把实际命中的窗口写进 trace：匹配规则写错时，这一行是最快的线索。
        trace.push(("目标窗口", "命中", format!("{} · 「{}」", t.exe, t.title)));
    }

    // 第 1 层：GSMTC。它不依赖窗口句柄，所以即使窗口没解析出来也值得一试。
    if cfg.strategies.gsmtc {
        let out = gsmtc::toggle(&cfg.target, target.as_ref(), cfg.follow_recent_session);
        trace.push(("GSMTC 媒体会话", out.label(), out.detail().to_string()));
        if let Outcome::Applied(detail) = out {
            return finish(true, Some("GSMTC 媒体会话"), detail, started, trace, false);
        }
    }

    // 后面三层都需要一个具体的 HWND。
    let Some(target) = target else {
        let msg = if cfg.target.is_empty() {
            "尚未设置目标窗口".to_string()
        } else {
            format!("没有窗口匹配规则 [{}]", cfg.target.describe())
        };
        return finish(false, None, msg, started, trace, false);
    };

    if cfg.strategies.appcommand {
        let out = messages::appcommand_play_pause(&target);
        trace.push(("WM_APPCOMMAND", out.label(), out.detail().to_string()));
        if let Outcome::Applied(detail) = out {
            return finish(true, Some("WM_APPCOMMAND"), detail, started, trace, false);
        }
    }

    if cfg.strategies.postmessage {
        let out = messages::post_space(&target);
        trace.push(("PostMessage 按键", out.label(), out.detail().to_string()));
        if let Outcome::Applied(detail) = out {
            return finish(true, Some("PostMessage 按键"), detail, started, trace, false);
        }
    }

    if cfg.strategies.focus_sendinput {
        let out = fallback::focus_and_send_space(&target, cfg.focus_settle_ms);
        trace.push(("抢焦点 SendInput", out.label(), out.detail().to_string()));
        if let Outcome::Applied(detail) = out {
            return finish(true, Some("抢焦点 SendInput"), detail, started, trace, true);
        }
    }

    finish(
        false,
        None,
        "所有已启用的策略都未生效".to_string(),
        started,
        trace,
        false,
    )
}

fn finish(
    ok: bool,
    winner: Option<&'static str>,
    detail: String,
    started: Instant,
    trace: Vec<(&'static str, &'static str, String)>,
    intrusive: bool,
) -> ActionReport {
    ActionReport {
        ok,
        winner,
        detail,
        elapsed: started.elapsed(),
        trace,
        intrusive,
    }
}

/// 把配置里的 exe 名规整成用于模糊比对的形式："chrome.exe" -> "chrome"。
pub(crate) fn exe_stem(spec: &TargetSpec) -> Option<String> {
    let exe = spec.exe.as_deref()?.trim().to_lowercase();
    if exe.is_empty() {
        return None;
    }
    Some(exe.strip_suffix(".exe").unwrap_or(&exe).to_string())
}

pub(crate) fn target_exe_stem(t: &ResolvedTarget) -> String {
    let e = t.exe.to_lowercase();
    e.strip_suffix(".exe").unwrap_or(&e).to_string()
}
