//! 「最近活跃的播放器」—— 让热键跟着**你最近动过的那个视频**走。
//!
//! ## 要解决的问题
//!
//! 两个浏览器同时开着视频时，[`super::gsmtc`] 的「唯一在播放」规则失效（有两个在播），
//! 命中权就全交给系统的 `GetCurrentSession`。那个"系统当前会话"由 Windows 自己决定、
//! 有出名的粘性，会赖在先注册的会话上——表现为「我刚在 Edge 点了播放，
//! Alt+Q 却还在暂停 Chrome」。先后顺序只能自己判。
//!
//! ## 判据：`TimelineProperties.LastUpdatedTime`
//!
//! 每个媒体会话都带一个"播放位置信息最后一次被刷新的时刻"。实测（Chromium 系）：
//!
//! * 手动播放、手动暂停、拖进度条 → **会刷新**
//! * 一直播着不动 → **纹丝不动**，不会随播放进度自己往前跑
//! * 完全没碰 → 不动
//!
//! 所以它就是"这个会话上次被碰是什么时候"的直接答案，取最大的那个即可。
//!
//! ## 为什么不是比对播放状态
//!
//! 早先的实现是"把这一轮的播放状态和上一轮存的快照比一比，变了就算动过"。
//! 它有个致命盲区：**手动播放→再手动暂停**这种一来一回的操作，
//! 净状态和上一轮完全一样，比对结果是"没动过"，热键就跟不过去——
//! 而这恰恰是最常见的用法。时间戳没有这个问题，一来一回会刷新两次。
//!
//! 换成时间戳之后，整套快照机制（记历史状态、首轮播种、回填自己造成的变化）
//! 全部不再需要，这个模块变成**无状态的纯函数**，也因此可以完整单测。
//!
//! ## 已知边界
//!
//! 如果某个播放器压根不上报时间线信息（`touched_at` 恒为 0），这里认不出它，
//! 会退回 [`super::gsmtc`] 后面几级挑选规则。

/// 一个候选会话的判据。
pub struct Seen<'a> {
    /// 小写 AppUserModelId，只用于日志。
    pub aumid: &'a str,
    /// `TimelineProperties.LastUpdatedTime` 的原始值（FILETIME，100 纳秒）。
    /// 取不到时为 0，表示"这个播放器不上报"，不参与排序。
    pub touched_at: i64,
}

/// 最近被碰过的那个会话在 `seen` 里的下标。
///
/// 全部都是 0（没有任何播放器上报时间线）时返回 `None`，交给后面几级规则。
pub fn most_recent(seen: &[Seen<'_>]) -> Option<usize> {
    seen.iter()
        .enumerate()
        .filter(|(_, s)| s.touched_at > 0)
        .max_by_key(|(_, s)| s.touched_at)
        .map(|(i, _)| i)
}

/// 排查用的一行摘要：`msedge(最近) chrome(-52.1s)`，括号里是比最近的那个早多久。
pub fn describe(seen: &[Seen<'_>]) -> String {
    let newest = seen.iter().map(|s| s.touched_at).max().unwrap_or(0);
    seen.iter()
        .map(|s| {
            if s.touched_at <= 0 {
                format!("{}(无时间线)", s.aumid)
            } else if s.touched_at == newest {
                format!("{}(最近)", s.aumid)
            } else {
                format!("{}({:.1}s前)", s.aumid, behind_secs(s.touched_at, newest))
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// `touched_at` 比 `newest` 早多少秒。FILETIME 的单位是 100 纳秒。
pub fn behind_secs(touched_at: i64, newest: i64) -> f32 {
    (newest - touched_at) as f32 / 1e7
}

#[cfg(test)]
mod tests {
    use super::{behind_secs, most_recent, Seen};

    fn seen<'a>(rows: &'a [(&'a str, i64)]) -> Vec<Seen<'a>> {
        rows.iter()
            .map(|(aumid, touched_at)| Seen {
                aumid,
                touched_at: *touched_at,
            })
            .collect()
    }

    /// 两个浏览器都暂停着，跟最后被碰的那个。
    #[test]
    fn picks_the_latest_touched() {
        let s = seen(&[("chrome", 100), ("msedge", 200)]);
        assert_eq!(most_recent(&s), Some(1));
    }

    /// 这就是实测里失败的场景：Chrome 手动播了又停（时间戳被刷新两次，
    /// 净播放状态却没变），此后热键必须跟到 Chrome。
    #[test]
    fn manual_play_then_pause_wins_even_though_status_is_unchanged() {
        // Edge 在 200 被碰过；随后用户在 Chrome 上手动播放(300)又暂停(400)
        let s = seen(&[("chrome", 400), ("msedge", 200)]);
        assert_eq!(most_recent(&s), Some(0));
    }

    /// 正在播放的会话不会因为"还在播"就一直刷新时间戳，
    /// 所以刚被碰过的暂停会话可以赢过一个播了很久的会话。
    #[test]
    fn a_long_playing_session_does_not_keep_winning() {
        let s = seen(&[("chrome", 100), ("msedge", 500)]);
        assert_eq!(most_recent(&s), Some(1));
    }

    #[test]
    fn ignores_sessions_without_timeline() {
        let s = seen(&[("weird_player", 0), ("chrome", 50)]);
        assert_eq!(most_recent(&s), Some(1));
    }

    #[test]
    fn none_when_nobody_reports_a_timeline() {
        let s = seen(&[("a", 0), ("b", 0)]);
        assert_eq!(most_recent(&s), None);
    }

    #[test]
    fn single_session_is_trivially_the_most_recent() {
        let s = seen(&[("chrome", 7)]);
        assert_eq!(most_recent(&s), Some(0));
    }

    #[test]
    fn behind_secs_converts_filetime_ticks() {
        // 1 秒 = 1e7 个 100 纳秒
        assert!((behind_secs(0, 10_000_000) - 1.0).abs() < 1e-6);
    }
}
