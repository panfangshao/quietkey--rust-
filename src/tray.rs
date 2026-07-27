//! 托盘图标与菜单。
//!
//! Windows 上托盘图标必须在拥有消息循环的那个线程上创建——这里就是 eframe
//! 的事件循环线程，所以构造过程放在 `App::new` 里做。
//!
//! 事件走 `set_event_handler` 主动唤醒 UI：托盘的消息由 winit 的消息泵分发，
//! 但窗口隐藏时 egui 不会自己重绘，不显式 `request_repaint` 就永远读不到菜单
//! 事件——表现为"点了托盘菜单没反应"。
//!
//! **关键陷阱**：`tray-icon` / `muda` 里 `set_event_handler(Some(..))` 一旦装上，
//! 事件就**只**走 handler，不再进 `TrayIconEvent::receiver()` / `MenuEvent::receiver()`
//! 那两个全局 channel（见 tray-icon 源码 `TrayIconEvent::send`）。
//! 所以 handler 必须把事件转存进本模块的队列，UI 侧改从 [`drain_tray`] /
//! [`drain_menu`] 取——否则托盘菜单点了完全没反应（连"退出"都失效）。

use std::collections::VecDeque;
use std::sync::{Mutex, OnceLock};

use tray_icon::menu::{Menu, MenuEvent, MenuItem, PredefinedMenuItem};
use tray_icon::{Icon, TrayIcon, TrayIconBuilder, TrayIconEvent};

use crate::icon;

pub struct Tray {
    /// 必须持有：drop 掉图标就从托盘区消失了。
    _icon: TrayIcon,
    pub show_id: tray_icon::menu::MenuId,
    pub toggle_id: tray_icon::menu::MenuId,
    pub quit_id: tray_icon::menu::MenuId,
}

pub fn build(tooltip: &str) -> anyhow::Result<Tray> {
    let menu = Menu::new();
    let show = MenuItem::new("打开设置", true, None);
    let toggle = MenuItem::new("立即播放/暂停", true, None);
    let quit = MenuItem::new("退出", true, None);

    menu.append(&show)?;
    menu.append(&toggle)?;
    menu.append(&PredefinedMenuItem::separator())?;
    menu.append(&quit)?;

    let icon = TrayIconBuilder::new()
        .with_menu(Box::new(menu))
        .with_tooltip(tooltip)
        .with_icon(make_icon())
        .build()?;

    Ok(Tray {
        _icon: icon,
        show_id: show.id().clone(),
        toggle_id: toggle.id().clone(),
        quit_id: quit.id().clone(),
    })
}

fn tray_queue() -> &'static Mutex<VecDeque<TrayIconEvent>> {
    static Q: OnceLock<Mutex<VecDeque<TrayIconEvent>>> = OnceLock::new();
    Q.get_or_init(Default::default)
}

fn menu_queue() -> &'static Mutex<VecDeque<MenuEvent>> {
    static Q: OnceLock<Mutex<VecDeque<MenuEvent>>> = OnceLock::new();
    Q.get_or_init(Default::default)
}

/// 接管托盘/菜单事件，转存进本模块队列并唤醒 egui 重绘。
///
/// 两件事缺一不可：不转存则事件被 handler 吞掉（channel 收不到），
/// 不 `request_repaint` 则窗口隐藏时 egui 不跑帧、队列没人取。
pub fn wire_wakeup(ctx: &egui::Context) {
    let c = ctx.clone();
    MenuEvent::set_event_handler(Some(move |ev| {
        push(menu_queue(), ev);
        c.request_repaint();
    }));
    let c = ctx.clone();
    TrayIconEvent::set_event_handler(Some(move |ev| {
        push(tray_queue(), ev);
        c.request_repaint();
    }));
}

pub fn drain_tray() -> Vec<TrayIconEvent> {
    drain(tray_queue())
}

pub fn drain_menu() -> Vec<MenuEvent> {
    drain(menu_queue())
}

/// 队列操作一律用 `into_inner` 兜住中毒锁：handler 里若有过 panic，
/// 托盘就此彻底哑掉（连退出都点不动）比丢一条事件严重得多。
fn push<T>(q: &Mutex<VecDeque<T>>, ev: T) {
    q.lock().unwrap_or_else(|e| e.into_inner()).push_back(ev);
}

fn drain<T>(q: &Mutex<VecDeque<T>>) -> Vec<T> {
    q.lock().unwrap_or_else(|e| e.into_inner()).drain(..).collect()
}

/// 程序化生成托盘图标，与窗口图标、.exe 资源图标同源（见 `src/icon.rs`）。
fn make_icon() -> Icon {
    const S: u32 = 32;
    Icon::from_rgba(icon::rgba(S), S, S).expect("内置图标数据有误")
}
