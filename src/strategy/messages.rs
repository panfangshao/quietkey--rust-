//! 第 2、3 层：直接向目标窗口投递消息，全程不碰前台焦点。

use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};
use windows::Win32::System::Threading::{AttachThreadInput, GetCurrentThreadId};
use windows::Win32::UI::Input::KeyboardAndMouse::GetFocus;
use windows::Win32::UI::WindowsAndMessaging::{
    GetWindowThreadProcessId, PostMessageW, SendMessageTimeoutW, SMTO_ABORTIFHUNG, SMTO_BLOCK,
    WM_APPCOMMAND, WM_KEYDOWN, WM_KEYUP,
};

use super::Outcome;
use crate::win::ResolvedTarget;

/// `APPCOMMAND_MEDIA_PLAY_PAUSE`，来自 winuser.h。
const APPCOMMAND_MEDIA_PLAY_PAUSE: isize = 14;
/// `FAPPCOMMAND_KEY`：表示这条命令来自键盘。
const FAPPCOMMAND_KEY: isize = 0;

const VK_SPACE: usize = 0x20;
/// 空格的键盘扫描码，填进 WM_KEYDOWN 的 lParam 里。
/// 有些程序（尤其是游戏引擎和自绘控件）会读扫描码而不是虚拟键码，
/// 填 0 会让这些程序直接忽略掉这条消息。
const SC_SPACE: isize = 0x39;

/// 第 2 层：`WM_APPCOMMAND` 定向投递。
///
/// 这是"多媒体键"走的那条路——键盘上的播放/暂停键按下时，系统就是给前台
/// 窗口发这条消息。它可以点对点投给任意 HWND，不需要焦点。VLC、PotPlayer、
/// foobar2000 这类播放器都实现了它。
///
/// 用 `SendMessageTimeoutW` 而不是 `PostMessageW`：前者能拿到窗口过程的返回值。
/// 按 MSDN，处理了 `WM_APPCOMMAND` 的程序应返回 TRUE，而 `DefWindowProc`
/// 会往上传并最终返回 0。这样我们**能够区分"对方接住了"和"对方无视了"**，
/// 从而正确地降级到下一层，而不是在这里假装成功。
///
/// 超时设 120ms 并带 `SMTO_ABORTIFHUNG`：目标程序卡死时不会把我们一起拖住。
pub fn appcommand_play_pause(target: &ResolvedTarget) -> Outcome {
    unsafe {
        let lparam = LPARAM((APPCOMMAND_MEDIA_PLAY_PAUSE | FAPPCOMMAND_KEY) << 16);
        let wparam = WPARAM(target.hwnd.0 as usize);
        let mut result: usize = 0;

        let ok = SendMessageTimeoutW(
            target.hwnd,
            WM_APPCOMMAND,
            wparam,
            lparam,
            SMTO_ABORTIFHUNG | SMTO_BLOCK,
            120,
            Some(&mut result),
        );

        if ok.0 == 0 {
            return Outcome::NotApplicable("窗口无响应或超时".to_string());
        }
        if result != 0 {
            Outcome::Applied(format!("{} 接受了播放/暂停命令", target.exe))
        } else {
            Outcome::NotApplicable(format!("{} 未处理 WM_APPCOMMAND", target.exe))
        }
    }
}

/// 第 3 层：把空格键的 `WM_KEYDOWN` / `WM_KEYUP` 直接投给目标窗口。
///
/// 适用于经典 Win32 消息循环的原生播放器。**对浏览器里的网页视频无效**——
/// Chromium 的键盘输入不走顶层窗口的消息队列，合成消息会被丢弃。
///
/// 关键细节：不能直接投给顶层 HWND，绝大多数播放器的键盘处理在内部的
/// 渲染子窗口上。借 `AttachThreadInput` 短暂挂到目标线程的输入队列上，
/// 就能用 `GetFocus` 问出它自己认为的焦点子窗口，再投给那个句柄。
///
/// 局限：`PostMessageW` 是异步的，只表示"消息塞进队列了"，无法确认对方
/// 是否真的响应。所以这一层返回的 Applied 带有"无法确认"的字样；
/// 如果它把不该拦的情况拦下了，可以在设置里单独关掉这一层。
pub fn post_space(target: &ResolvedTarget) -> Outcome {
    unsafe {
        let dest = focused_child(target.hwnd).unwrap_or(target.hwnd);

        let down = LPARAM(1 | (SC_SPACE << 16));
        // bit30 = 之前的按键状态，bit31 = 转换状态，抬起时都要置 1。
        let up = LPARAM(1 | (SC_SPACE << 16) | (1 << 30) | (1 << 31));

        let a = PostMessageW(Some(dest), WM_KEYDOWN, WPARAM(VK_SPACE), down);
        let b = PostMessageW(Some(dest), WM_KEYUP, WPARAM(VK_SPACE), up);

        if a.is_ok() && b.is_ok() {
            let where_ = if dest == target.hwnd {
                "主窗口".to_string()
            } else {
                format!("子窗口 {:?}", dest.0)
            };
            Outcome::Applied(format!("空格已投递到 {} 的{}（无法确认是否响应）", target.exe, where_))
        } else {
            Outcome::Failed("PostMessage 投递失败".to_string())
        }
    }
}

/// 问出目标线程当前的焦点子窗口。
fn focused_child(hwnd: HWND) -> Option<HWND> {
    unsafe {
        let target_thread = GetWindowThreadProcessId(hwnd, None);
        if target_thread == 0 {
            return None;
        }
        let cur = GetCurrentThreadId();
        if target_thread == cur {
            return GetFocus().into_option();
        }

        let _ = AttachThreadInput(cur, target_thread, true);
        let focus = GetFocus();
        let _ = AttachThreadInput(cur, target_thread, false);

        focus.into_option()
    }
}

trait HwndExt {
    fn into_option(self) -> Option<HWND>;
}

impl HwndExt for HWND {
    fn into_option(self) -> Option<HWND> {
        if self.is_invalid() {
            None
        } else {
            Some(self)
        }
    }
}
