//! 第 4 层：抢焦点 → SendInput → 立刻还原。**唯一会打断用户的一层。**
//!
//! 这就是原来 Python 版的做法，但把它做对了：
//!
//! * Python 版用三段 `time.sleep(0.5)` 硬等，整整 1.5 秒焦点在乱跳，
//!   期间敲下的字会散落到错误的窗口里——这正是"打字快就会串行"的根因。
//!   这里只在切换后等一个可配置的极短间隔（默认 30ms），够窗口管理器
//!   完成焦点转移即可。
//! * Python 版切走之后**再也不切回来**依赖第二次 `activate()`，中间那段
//!   时间用户的输入无处可去。这里在发完按键后立刻把前台还原成原来的窗口，
//!   而且记录的是「触发热键那一刻的前台窗口」，不是配置里的笔记窗口，
//!   所以你在哪个窗口按的热键，焦点就回到哪里。
//!
//! 即便如此，这一层仍然会让屏幕闪一下，所以默认是关闭的。
//! 只有在前三层都对你的播放器无效时才需要打开它。

use std::thread::sleep;
use std::time::Duration;

use super::Outcome;
use crate::win::{force_foreground, foreground_window, send_key, window_title, ResolvedTarget};

const VK_SPACE: u16 = 0x20;

pub fn focus_and_send_space(target: &ResolvedTarget, settle_ms: u64) -> Outcome {
    // 先记下"我是从哪儿按的热键"，这才是待会要还原的窗口。
    let origin = foreground_window();

    if origin == target.hwnd {
        // 目标已经在前台，直接发按键，不需要来回切。
        return if send_key(VK_SPACE) {
            Outcome::Applied(format!("{} 已在前台，直接发送空格", target.exe))
        } else {
            Outcome::Failed("SendInput 失败".to_string())
        };
    }

    if !force_foreground(target.hwnd) {
        return Outcome::Failed(format!("无法把 {} 切到前台", target.exe));
    }
    sleep(Duration::from_millis(settle_ms.clamp(0, 500)));

    let sent = send_key(VK_SPACE);

    // 无论按键是否发成功，都必须把焦点还回去，否则用户会卡在视频窗口里。
    let restored = if origin.is_invalid() {
        false
    } else {
        sleep(Duration::from_millis(settle_ms.clamp(0, 500)));
        force_foreground(origin)
    };

    if !sent {
        return Outcome::Failed("SendInput 失败".to_string());
    }

    let back = if restored {
        let t = window_title(origin);
        if t.is_empty() {
            "已还原焦点".to_string()
        } else {
            format!("焦点已还给「{}」", truncate(&t, 24))
        }
    } else {
        "但焦点还原失败".to_string()
    };

    Outcome::Applied(format!("切到 {} 发送空格，{back}", target.exe))
}

fn truncate(s: &str, max_chars: usize) -> String {
    if s.chars().count() <= max_chars {
        return s.to_string();
    }
    let head: String = s.chars().take(max_chars).collect();
    format!("{head}…")
}
