//! 第 1 层：Windows 全局媒体传输控制（GSMTC）。
//!
//! 这是「一次都不切窗口」的正解，也是这个项目相对 Python 版最大的改进。
//!
//! Chrome / Edge / Firefox 在播放视频时都会向系统注册一个媒体会话——就是
//! 按音量键时弹出的那个媒体浮层、锁屏上的播放控件背后的东西。我们直接枚举
//! 会话并调用 `TryTogglePlayPauseAsync`，全程：
//!
//! * 不需要焦点，不切窗口，不动前台
//! * 不需要窗口标题匹配（标题会随播放进度变，本来就不可靠）
//! * 窗口最小化、被完全遮挡都照样生效
//!
//! 注意：向浏览器顶层 HWND 投递合成的 `WM_KEYDOWN` 是**行不通**的，
//! Chromium 的输入走自己的 renderer 管线会直接把它丢掉。所以对网页视频来说，
//! 这一层不是「优化」，而是唯一可行的无感方案。

use windows::Media::Control::{
    GlobalSystemMediaTransportControlsSession as Session,
    GlobalSystemMediaTransportControlsSessionManager as SessionManager,
    GlobalSystemMediaTransportControlsSessionPlaybackStatus as PlaybackStatus,
};

use super::{exe_stem, recency, Outcome};
use crate::config::TargetSpec;
use crate::win::ResolvedTarget;

/// 尝试通过媒体会话切换播放状态。
///
/// 目标会话的挑选顺序（第一个命中即用）：
/// 1. AppUserModelId 与配置的进程名吻合的会话——**显式配置永远最优先**；
/// 2. 最近活跃的会话（见 [`session_watch`]，`follow_recent_session` 控制）——
///    多个浏览器同时在播时靠它跟住"你最近动过的那一个"；
/// 3. 当前**有且只有一个**正在播放的会话——「只开了一个视频」的日常场景，省去配置；
/// 4. 系统当前会话（`GetCurrentSession`，即多媒体键控制的那一个）；
/// 5. 系统里总共只有一个会话时，就是它。
///
/// 第 4、5 条是「能暂停、不能恢复」的解药：视频一旦被我们暂停，
/// 正在播放的会话数就变成 0，只靠第 3 条会彻底找不回目标，
/// 于是掉到对浏览器无效的第 2/3 层，表现为再按一次没反应。
pub fn toggle(
    spec: &TargetSpec,
    resolved: Option<&ResolvedTarget>,
    follow_recent: bool,
) -> Outcome {
    let manager = match SessionManager::RequestAsync().and_then(|op| op.join()) {
        Ok(m) => m,
        Err(e) => return Outcome::NotApplicable(format!("媒体会话管理器不可用: {}", short(&e))),
    };

    let sessions = match manager.GetSessions() {
        Ok(s) => s,
        Err(e) => return Outcome::NotApplicable(format!("枚举媒体会话失败: {}", short(&e))),
    };

    let all = collect(sessions);
    if all.is_empty() {
        return Outcome::NotApplicable("系统当前没有任何媒体会话".to_string());
    }
    // 排查"为什么控的是它"时，这一行是最直接的证据。
    // 刻意用 INFO：只在触发时打一行，却是唯一能回答这个问题的信息。
    log::info!("会话活跃度: {}", recency::describe(&seen_rows(&all)));

    // 配置里的进程名，以及实际解析到的窗口的进程名，都拿来做匹配依据。
    let mut wanted: Vec<String> = Vec::new();
    if let Some(stem) = exe_stem(spec) {
        wanted.push(stem);
    }
    if let Some(t) = resolved {
        wanted.push(super::target_exe_stem(t));
    }

    // 系统当前会话：多媒体键作用的就是它。暂停之后它仍然指向刚才那个播放器，
    // 所以是"恢复播放"唯一还站得住的定位依据。
    let current_aumid = manager
        .GetCurrentSession()
        .ok()
        .and_then(|s| s.SourceAppUserModelId().ok())
        .map(|h| h.to_string_lossy().to_lowercase())
        .unwrap_or_default();

    let picked = all
        .iter()
        .position(|f| {
            wanted
                .iter()
                .any(|w| !w.is_empty() && (f.aumid.contains(w) || w.contains(f.aumid.as_str())))
        })
        .map(|i| (i, "配置匹配"))
        .or_else(|| {
            // 按会话逐个比时间戳，不按进程名归并——所以一个浏览器开多个视频标签页、
            // 各自注册一个会话时，也能精确挑到你最近碰过的那一个。
            follow_recent
                .then(|| recency::most_recent(&seen_rows(&all)))
                .flatten()
                .map(|i| (i, "最近活跃"))
        })
        .or_else(|| only_playing(&all).map(|i| (i, "唯一在播放")))
        .or_else(|| {
            all.iter()
                .position(|f| !current_aumid.is_empty() && f.aumid == current_aumid)
                .map(|i| (i, "系统当前会话"))
        })
        .or_else(|| (all.len() == 1).then_some((0, "唯一会话")));

    let Some((idx, route)) = picked else {
        return Outcome::NotApplicable(format!(
            "{} 个会话中没有匹配项（{}）",
            all.len(),
            all.iter()
                .map(|f| f.aumid.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        ));
    };
    let Found {
        session,
        aumid,
        playing,
        ..
    } = &all[idx];
    let direction = if *playing { "已暂停" } else { "已播放" };

    let toggle_ok = session
        .GetPlaybackInfo()
        .and_then(|i| i.Controls())
        .and_then(|c| c.IsPlayPauseToggleEnabled())
        .unwrap_or(true);

    // 先走 Toggle；它被会话声明为不可用、或调用后被拒绝时，
    // 再按当前播放状态退到明确的 Play / Pause——有些播放器（尤其是网页视频）
    // 只在暂停态下开放 Play，Toggle 一直报 false，不退这一步就永远恢复不了。
    // 我们自己造成的这次切换同样会刷新会话的时间线时间戳，
    // 所以"暂停之后再按一次恢复"天然落在同一个会话上，不需要额外记账。
    if toggle_ok {
        match session.TryTogglePlayPauseAsync().and_then(|op| op.join()) {
            Ok(true) => return Outcome::Applied(format!("{aumid} → {direction}（{route}）")),
            Ok(false) => {}
            Err(e) => {
                return Outcome::Failed(format!("切换 {aumid} 失败: {}", short(&e)));
            }
        }
    }

    match explicit_play_pause(session, *playing) {
        Ok(true) => Outcome::Applied(format!("{aumid} → {direction}（{route}·显式）")),
        Ok(false) => Outcome::Failed(format!("会话 {aumid} 拒绝了播放/暂停请求")),
        Err(e) => Outcome::Failed(format!("切换 {aumid} 失败: {}", short(&e))),
    }
}

/// 枚举到的一个会话。
struct Found {
    session: Session,
    /// 小写 AppUserModelId。
    aumid: String,
    playing: bool,
    /// `TimelineProperties.LastUpdatedTime`，取不到时为 0。
    touched_at: i64,
}

fn collect(sessions: impl IntoIterator<Item = Session>) -> Vec<Found> {
    sessions
        .into_iter()
        .map(|session| {
            let aumid = session
                .SourceAppUserModelId()
                .map(|h| h.to_string_lossy().to_lowercase())
                .unwrap_or_default();
            let playing = session
                .GetPlaybackInfo()
                .and_then(|i| i.PlaybackStatus())
                .map(|st| st == PlaybackStatus::Playing)
                .unwrap_or(false);
            // 播放位置信息最后一次被刷新的时刻（FILETIME，100 纳秒为单位）。
            // 它比播放状态更能反映"这个会话被碰过"：手动播了又停这种
            // **净状态零变化**的操作，状态比对完全看不见，但它会被刷新。
            let touched_at = session
                .GetTimelineProperties()
                .and_then(|t| t.LastUpdatedTime())
                .map(|d| d.UniversalTime)
                .unwrap_or(0);
            Found {
                session,
                aumid,
                playing,
                touched_at,
            }
        })
        .collect()
}

/// 注意：**不做任何过滤**，下标要和 `all` 一一对应，
/// [`recency::most_recent`] 返回的下标才能直接拿来索引 `all`。
fn seen_rows(all: &[Found]) -> Vec<recency::Seen<'_>> {
    all.iter()
        .map(|f| recency::Seen {
            aumid: &f.aumid,
            touched_at: f.touched_at,
        })
        .collect()
}

/// 有且只有一个正在播放的会话时返回它的下标。
fn only_playing(all: &[Found]) -> Option<usize> {
    let mut it = all.iter().enumerate().filter(|(_, f)| f.playing);
    match (it.next(), it.next()) {
        (Some((i, _)), None) => Some(i),
        _ => None,
    }
}

fn explicit_play_pause(session: &Session, playing: bool) -> windows::core::Result<bool> {
    if playing {
        session.TryPauseAsync()?.join()
    } else {
        session.TryPlayAsync()?.join()
    }
}

/// 诊断面板里的一行媒体会话。
pub struct SessionInfo {
    pub aumid: String,
    pub playing: bool,
    /// 是否被判定为「最近活跃」，也就是热键会跟过去的那一个。
    pub recent: bool,
    /// 上次被碰的时刻比最近的那个早多少秒。0 表示它就是最近的。
    pub behind_secs: f32,
    /// 这个播放器是否上报时间线信息。不上报的话参与不了活跃度比较。
    pub has_timeline: bool,
}

/// 列出当前所有媒体会话，供 UI 诊断面板和 `--list-sessions` 展示。
pub fn list_sessions() -> Vec<SessionInfo> {
    let Ok(manager) = SessionManager::RequestAsync().and_then(|op| op.join()) else {
        return Vec::new();
    };
    let Ok(sessions) = manager.GetSessions() else {
        return Vec::new();
    };
    let all = collect(sessions);
    let recent = recency::most_recent(&seen_rows(&all));
    let newest = all.iter().map(|f| f.touched_at).max().unwrap_or(0);
    all.iter()
        .enumerate()
        .map(|(i, f)| SessionInfo {
            aumid: f.aumid.clone(),
            playing: f.playing,
            recent: recent == Some(i),
            behind_secs: recency::behind_secs(f.touched_at, newest),
            has_timeline: f.touched_at > 0,
        })
        .collect()
}

fn short(e: &windows::core::Error) -> String {
    let m = e.message();
    if m.is_empty() {
        format!("{:?}", e.code())
    } else {
        m
    }
}
