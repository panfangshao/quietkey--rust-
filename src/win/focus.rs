//! 前台窗口的抢占与还原——只给策略链最后一层兜底用。

use windows::Win32::Foundation::HWND;
use windows::Win32::System::Threading::{AttachThreadInput, GetCurrentThreadId};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    SendInput, SetFocus, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYBD_EVENT_FLAGS,
    KEYEVENTF_KEYUP, VIRTUAL_KEY,
};
use windows::Win32::UI::WindowsAndMessaging::{
    GetForegroundWindow, GetWindowThreadProcessId, IsIconic, SetForegroundWindow, ShowWindow,
    SW_RESTORE,
};

/// 强制把某个窗口切到前台。
///
/// `SetForegroundWindow` 单独调用通常会失败——Windows 有前台锁定机制，
/// 只允许当前前台进程、或刚响应过用户输入的进程去改前台窗口。
/// 标准绕法是先用 `AttachThreadInput` 把自己的线程输入队列挂到当前前台
/// 线程上，借它的身份完成切换，之后立刻解绑。
pub fn force_foreground(hwnd: HWND) -> bool {
    unsafe {
        if hwnd.is_invalid() {
            return false;
        }
        if IsIconic(hwnd).as_bool() {
            let _ = ShowWindow(hwnd, SW_RESTORE);
        }

        let fg = GetForegroundWindow();
        if fg == hwnd {
            return true;
        }

        let cur_thread = GetCurrentThreadId();
        let fg_thread = if fg.is_invalid() {
            0
        } else {
            GetWindowThreadProcessId(fg, None)
        };

        let attached = fg_thread != 0 && fg_thread != cur_thread;
        if attached {
            let _ = AttachThreadInput(cur_thread, fg_thread, true);
        }

        let ok = SetForegroundWindow(hwnd).as_bool();
        let _ = SetFocus(Some(hwnd));

        if attached {
            let _ = AttachThreadInput(cur_thread, fg_thread, false);
        }
        ok
    }
}

pub fn foreground_window() -> HWND {
    unsafe { GetForegroundWindow() }
}

/// 向**当前前台窗口**发送一次按键（按下+抬起）。
///
/// `SendInput` 走的是系统输入队列，任何程序都拦不住——代价是它只认前台，
/// 所以必须先抢焦点。这也正是这一层会让屏幕闪一下的原因。
pub fn send_key(vk: u16) -> bool {
    unsafe {
        let inputs = [
            INPUT {
                r#type: INPUT_KEYBOARD,
                Anonymous: INPUT_0 {
                    ki: KEYBDINPUT {
                        wVk: VIRTUAL_KEY(vk),
                        wScan: 0,
                        dwFlags: KEYBD_EVENT_FLAGS(0),
                        time: 0,
                        dwExtraInfo: 0,
                    },
                },
            },
            INPUT {
                r#type: INPUT_KEYBOARD,
                Anonymous: INPUT_0 {
                    ki: KEYBDINPUT {
                        wVk: VIRTUAL_KEY(vk),
                        wScan: 0,
                        dwFlags: KEYEVENTF_KEYUP,
                        time: 0,
                        dwExtraInfo: 0,
                    },
                },
            },
        ];
        let sent = SendInput(&inputs, std::mem::size_of::<INPUT>() as i32);
        sent == inputs.len() as u32
    }
}
