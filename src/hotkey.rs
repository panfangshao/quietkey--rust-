//! 全局热键：基于 `global-hotkey`，底层是 Win32 的 `RegisterHotKey`。
//!
//! 为什么不用 `rdev` 那种低级键盘钩子（`WH_KEYBOARD_LL`）：
//!
//! * **杀软**：全局低级键盘钩子是 keylogger 的教科书特征，未签名的自编译
//!   程序挂上去很容易被 Defender 拦掉。
//! * **全系统输入延迟**：钩子回调跑在本进程里，且受 `LowLevelHooksTimeout`
//!   约束（默认约 300ms）。我们在回调里做窗口查找这类可能阻塞的事，会拖慢
//!   **整个系统**的键盘响应——对"一边打字做笔记"的场景是致命的。
//! * **隐私**：钩子能看到用户敲的每一个字符；`RegisterHotKey` 只能看到注册
//!   的那一个组合键。
//!
//! 另外 `RegisterHotKey` 注册的组合键会被系统独占消费，不会再漏给前台窗口——
//! 这正好修掉了 Python 版用 `keyboard` 库时"Alt+Q 同时被笔记软件收到"的隐患。
//!
//! 代价：组合键可能已被别的程序占用，注册会直接失败。所以 [`HotkeyService::apply`]
//! 会把错误如实抛给 UI 提示用户换一个，而不是静默失败。

use anyhow::{anyhow, Result};
use global_hotkey::hotkey::{Code, HotKey, Modifiers};
use global_hotkey::GlobalHotKeyManager;

pub struct HotkeyService {
    manager: GlobalHotKeyManager,
    current: Option<HotKey>,
    /// 当前实际注册成功的组合键描述，UI 用它显示真实状态。
    pub registered: Option<String>,
}

impl HotkeyService {
    pub fn new() -> Result<Self> {
        let manager = GlobalHotKeyManager::new()
            .map_err(|e| anyhow!("初始化热键管理器失败: {e}"))?;
        Ok(Self {
            manager,
            current: None,
            registered: None,
        })
    }

    /// 注册新的组合键，成功后自动注销旧的。
    ///
    /// 先注销再注册，否则把热键改成它自己时会撞上"已被占用"。
    pub fn apply(&mut self, spec: &str) -> Result<()> {
        let hotkey = parse_hotkey(spec)?;

        if let Some(old) = self.current.take() {
            let _ = self.manager.unregister(old);
        }
        self.registered = None;

        self.manager.register(hotkey).map_err(|e| {
            anyhow!("注册 {spec} 失败（多半是已被其他程序占用，换一个组合键试试）: {e}")
        })?;

        self.current = Some(hotkey);
        self.registered = Some(spec.to_string());
        Ok(())
    }

}

impl Drop for HotkeyService {
    fn drop(&mut self) {
        if let Some(h) = self.current.take() {
            let _ = self.manager.unregister(h);
        }
    }
}

/// 解析 "Alt+KeyQ"、"Ctrl+Shift+Space" 这样的描述。
///
/// 这里用显式映射表而不是依赖 `Code` 的 `FromStr`，是为了能给出中文错误提示，
/// 并且顺手接受 "Q"/"KeyQ"、"Ctrl"/"Control" 这类等价写法。
pub fn parse_hotkey(spec: &str) -> Result<HotKey> {
    let mut mods = Modifiers::empty();
    let mut code: Option<Code> = None;

    for raw in spec.split('+').map(str::trim).filter(|p| !p.is_empty()) {
        match raw.to_ascii_lowercase().as_str() {
            "alt" => mods |= Modifiers::ALT,
            "ctrl" | "control" => mods |= Modifiers::CONTROL,
            "shift" => mods |= Modifiers::SHIFT,
            "win" | "super" | "meta" | "cmd" => mods |= Modifiers::META,
            other => {
                if code.is_some() {
                    return Err(anyhow!("组合键里只能有一个主键，多出了 \"{raw}\""));
                }
                code = Some(parse_code(other).ok_or_else(|| anyhow!("无法识别的按键 \"{raw}\""))?);
            }
        }
    }

    let code = code.ok_or_else(|| anyhow!("组合键缺少主键，例如 Alt+Q 里的 Q"))?;
    if mods.is_empty() {
        return Err(anyhow!("请至少带一个修饰键（Alt / Ctrl / Shift / Win），否则会和正常打字冲突"));
    }
    Ok(HotKey::new(Some(mods), code))
}

fn parse_code(s: &str) -> Option<Code> {
    // 允许省略 "key"/"digit" 前缀："q" 与 "keyq" 等价。
    let s = s.strip_prefix("key").unwrap_or(s);
    let s = s.strip_prefix("digit").unwrap_or(s);

    Some(match s {
        "a" => Code::KeyA, "b" => Code::KeyB, "c" => Code::KeyC, "d" => Code::KeyD,
        "e" => Code::KeyE, "f" => Code::KeyF, "g" => Code::KeyG, "h" => Code::KeyH,
        "i" => Code::KeyI, "j" => Code::KeyJ, "k" => Code::KeyK, "l" => Code::KeyL,
        "m" => Code::KeyM, "n" => Code::KeyN, "o" => Code::KeyO, "p" => Code::KeyP,
        "q" => Code::KeyQ, "r" => Code::KeyR, "s" => Code::KeyS, "t" => Code::KeyT,
        "u" => Code::KeyU, "v" => Code::KeyV, "w" => Code::KeyW, "x" => Code::KeyX,
        "y" => Code::KeyY, "z" => Code::KeyZ,
        "0" => Code::Digit0, "1" => Code::Digit1, "2" => Code::Digit2, "3" => Code::Digit3,
        "4" => Code::Digit4, "5" => Code::Digit5, "6" => Code::Digit6, "7" => Code::Digit7,
        "8" => Code::Digit8, "9" => Code::Digit9,
        "f1" => Code::F1, "f2" => Code::F2, "f3" => Code::F3, "f4" => Code::F4,
        "f5" => Code::F5, "f6" => Code::F6, "f7" => Code::F7, "f8" => Code::F8,
        "f9" => Code::F9, "f10" => Code::F10, "f11" => Code::F11, "f12" => Code::F12,
        "space" => Code::Space,
        "enter" | "return" => Code::Enter,
        "tab" => Code::Tab,
        "escape" | "esc" => Code::Escape,
        "backquote" | "`" => Code::Backquote,
        "minus" | "-" => Code::Minus,
        "equal" | "=" => Code::Equal,
        "bracketleft" | "[" => Code::BracketLeft,
        "bracketright" | "]" => Code::BracketRight,
        "backslash" | "\\" => Code::Backslash,
        "semicolon" | ";" => Code::Semicolon,
        "quote" | "'" => Code::Quote,
        "comma" | "," => Code::Comma,
        "period" | "." => Code::Period,
        "slash" | "/" => Code::Slash,
        "insert" => Code::Insert,
        "delete" | "del" => Code::Delete,
        "home" => Code::Home,
        "end" => Code::End,
        "pageup" => Code::PageUp,
        "pagedown" => Code::PageDown,
        "up" | "arrowup" => Code::ArrowUp,
        "down" | "arrowdown" => Code::ArrowDown,
        "left" | "arrowleft" => Code::ArrowLeft,
        "right" | "arrowright" => Code::ArrowRight,
        _ => return None,
    })
}

/// 把 egui 捕获到的按键还原成我们的字符串格式，供 UI 的"录制热键"用。
pub fn spec_from_egui(key: egui::Key, mods: egui::Modifiers) -> Option<String> {
    let mut parts = Vec::new();
    if mods.ctrl {
        parts.push("Ctrl");
    }
    if mods.alt {
        parts.push("Alt");
    }
    if mods.shift {
        parts.push("Shift");
    }
    if mods.command && !mods.ctrl {
        parts.push("Win");
    }
    if parts.is_empty() {
        return None;
    }
    let name = key.name(); // egui 给的就是 "Q" / "F9" / "Space" 这类名字
    parts.push(name);
    Some(parts.join("+"))
}
