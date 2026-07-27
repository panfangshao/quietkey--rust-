//! 枚举可见顶层窗口，并把配置里的匹配规则解析成具体的 HWND。

use windows::core::{BOOL, PWSTR};
use windows::Win32::Foundation::{CloseHandle, HANDLE, HWND, LPARAM};
use windows::Win32::Graphics::Dwm::{DwmGetWindowAttribute, DWMWA_CLOAKED};
use windows::Win32::System::Threading::{
    OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION,
};
use windows::Win32::UI::WindowsAndMessaging::{
    EnumWindows, GetClassNameW, GetWindow, GetWindowLongPtrW, GetWindowTextLengthW, GetWindowTextW,
    GetWindowThreadProcessId, IsWindowVisible, GWL_EXSTYLE, GW_OWNER, WS_EX_TOOLWINDOW,
};

use crate::config::TargetSpec;

/// `EnumWindows` 回调的返回值：非零表示继续枚举。
const CONTINUE: BOOL = BOOL(1);

/// 一个候选窗口的快照，供 UI 的「窗口拾取器」展示。
#[derive(Debug, Clone)]
pub struct WindowInfo {
    pub hwnd: isize,
    pub title: String,
    pub class_name: String,
    pub exe: String,
    pub pid: u32,
}

/// 已解析到的目标：热键触发时策略链拿到的就是它。
#[derive(Debug, Clone)]
pub struct ResolvedTarget {
    pub hwnd: HWND,
    pub exe: String,
    pub title: String,
}

/// 枚举当前所有「像样的」可见顶层窗口。
///
/// 过滤掉了三类噪音：无标题窗口、工具窗口（WS_EX_TOOLWINDOW，不出现在
/// Alt+Tab 里）、以及 DWM 标记为 cloaked 的窗口——后者是 UWP 应用留下的
/// 隐形空壳，不过滤的话列表里会混进一堆看不见的「幽灵窗口」。
pub fn list_windows() -> Vec<WindowInfo> {
    let mut out: Vec<WindowInfo> = Vec::new();
    unsafe {
        let _ = EnumWindows(
            Some(enum_proc),
            LPARAM(&mut out as *mut Vec<WindowInfo> as isize),
        );
    }
    out.sort_by(|a, b| {
        a.exe
            .to_lowercase()
            .cmp(&b.exe.to_lowercase())
            .then_with(|| a.title.to_lowercase().cmp(&b.title.to_lowercase()))
    });
    out
}

unsafe extern "system" fn enum_proc(hwnd: HWND, lparam: LPARAM) -> BOOL {
    let out = &mut *(lparam.0 as *mut Vec<WindowInfo>);

    if !IsWindowVisible(hwnd).as_bool() {
        return CONTINUE;
    }
    // 有 owner 的窗口通常是对话框/弹层，不是我们要控制的主窗口。
    if GetWindow(hwnd, GW_OWNER).map(|h| !h.is_invalid()).unwrap_or(false) {
        return CONTINUE;
    }
    let ex_style = GetWindowLongPtrW(hwnd, GWL_EXSTYLE) as u32;
    if ex_style & WS_EX_TOOLWINDOW.0 != 0 {
        return CONTINUE;
    }
    if is_cloaked(hwnd) {
        return CONTINUE;
    }

    let title = window_title(hwnd);
    if title.trim().is_empty() {
        return CONTINUE;
    }

    let mut pid = 0u32;
    GetWindowThreadProcessId(hwnd, Some(&mut pid));

    out.push(WindowInfo {
        hwnd: hwnd.0 as isize,
        title,
        class_name: window_class(hwnd),
        exe: process_exe_name(pid).unwrap_or_default(),
        pid,
    });
    CONTINUE
}

fn is_cloaked(hwnd: HWND) -> bool {
    unsafe {
        let mut cloaked: u32 = 0;
        let ok = DwmGetWindowAttribute(
            hwnd,
            DWMWA_CLOAKED,
            &mut cloaked as *mut u32 as *mut _,
            std::mem::size_of::<u32>() as u32,
        );
        ok.is_ok() && cloaked != 0
    }
}

pub fn window_title(hwnd: HWND) -> String {
    unsafe {
        let len = GetWindowTextLengthW(hwnd);
        if len <= 0 {
            return String::new();
        }
        let mut buf = vec![0u16; len as usize + 1];
        let n = GetWindowTextW(hwnd, &mut buf);
        String::from_utf16_lossy(&buf[..n as usize])
    }
}

pub fn window_class(hwnd: HWND) -> String {
    unsafe {
        let mut buf = [0u16; 256];
        let n = GetClassNameW(hwnd, &mut buf);
        String::from_utf16_lossy(&buf[..n as usize])
    }
}

/// 通过 PID 拿可执行文件名（不含路径）。
///
/// 用 `QueryFullProcessImageNameW` 而不是 `GetModuleBaseNameW`：前者只需要
/// `PROCESS_QUERY_LIMITED_INFORMATION` 权限，对付以更高完整性级别运行的
/// 进程时成功率明显更高。
pub fn process_exe_name(pid: u32) -> Option<String> {
    if pid == 0 {
        return None;
    }
    unsafe {
        let handle: HANDLE = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).ok()?;
        let mut buf = [0u16; 512];
        let mut size = buf.len() as u32;
        let res = QueryFullProcessImageNameW(
            handle,
            PROCESS_NAME_WIN32,
            PWSTR(buf.as_mut_ptr()),
            &mut size,
        );
        let _ = CloseHandle(handle);
        res.ok()?;
        let full = String::from_utf16_lossy(&buf[..size as usize]);
        Some(
            full.rsplit(['\\', '/'])
                .next()
                .unwrap_or(&full)
                .to_string(),
        )
    }
}

/// 按匹配规则解析出目标窗口。
///
/// 若有多个窗口命中，取标题最长的那个——经验上浏览器的正片标签页标题
/// 比「新标签页」这类空壳更长，命中率更高。
pub fn resolve_target(spec: &TargetSpec) -> Option<ResolvedTarget> {
    if spec.is_empty() {
        return None;
    }
    let exe = spec.exe.as_deref().unwrap_or("").to_lowercase();
    let title_sub = spec.title_contains.as_deref().unwrap_or("").to_lowercase();
    let class = spec.class_name.as_deref().unwrap_or("");

    let mut best: Option<WindowInfo> = None;
    for w in list_windows() {
        if !exe.is_empty() && w.exe.to_lowercase() != exe {
            continue;
        }
        if !title_sub.is_empty() && !w.title.to_lowercase().contains(&title_sub) {
            continue;
        }
        if !class.is_empty() && w.class_name != class {
            continue;
        }
        if best
            .as_ref()
            .map(|b| w.title.chars().count() > b.title.chars().count())
            .unwrap_or(true)
        {
            best = Some(w);
        }
    }

    best.map(|w| ResolvedTarget {
        hwnd: HWND(w.hwnd as *mut _),
        exe: w.exe,
        title: w.title,
    })
}
