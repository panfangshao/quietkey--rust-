// 发布版不要控制台窗口，否则双击启动会闪一个黑框。
// debug 版保留，方便看 log。
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;
mod config;
mod hotkey;
mod icon;
mod state;
mod strategy;
mod theme;
mod tray;
mod win;

use std::sync::atomic::AtomicBool;
use std::sync::Arc;

use global_hotkey::{GlobalHotKeyEvent, HotKeyState};
use windows::Win32::System::Com::CoIncrementMTAUsage;

use crate::config::Config;
use crate::hotkey::HotkeyService;
use crate::state::Shared;

fn main() -> eframe::Result<()> {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();

    let cfg = Config::load();

    // `quietkey.exe --list-sessions`：只读地列出系统当前的媒体会话，什么都不改。
    // 「某个播放器为什么控不到」的第一问永远是"它到底注册会话了没有"，
    // 这个命令是回答它最快的方式，也不会像 --test-once 那样把用户的视频切来切去。
    if std::env::args().skip(1).any(|a| a == "--list-sessions") {
        if let Err(e) = unsafe { CoIncrementMTAUsage() } {
            eprintln!("COM MTA 初始化失败，媒体会话策略将不可用: {e}");
        }
        let sessions = strategy::gsmtc::list_sessions();
        if sessions.is_empty() {
            println!("系统当前没有任何媒体会话。");
            println!("浏览器只有在播放【带声音、未静音、时长足够】的媒体时才会注册会话。");
        } else {
            println!("共 {} 个媒体会话：", sessions.len());
            for s in &sessions {
                let activity = if !s.has_timeline {
                    "不上报时间线".to_string()
                } else if s.recent {
                    "最近碰过".to_string()
                } else {
                    format!("{:.1} 秒前碰过", s.behind_secs)
                };
                println!(
                    "  {} {} {:<24} {}",
                    if s.playing { "▶ 播放中" } else { "⏸ 已暂停" },
                    if s.recent { "★" } else { "  " },
                    s.aumid,
                    activity
                );
            }
        }
        return Ok(());
    }

    // `quietkey.exe --test-once`：不开界面、不注册热键，直接跑一次策略链并打印
    // 逐层结果。排查「哪一层生效、为什么没生效」时比开 GUI 快得多。
    // 注意 release 构建没有控制台，这个参数只在 debug 构建下看得到输出。
    if std::env::args().skip(1).any(|a| a == "--test-once") {
        if let Err(e) = unsafe { CoIncrementMTAUsage() } {
            eprintln!("COM MTA 初始化失败，媒体会话策略将不可用: {e}");
        }
        let report = strategy::toggle_play_pause(&cfg);
        println!("结果: {}", report.summary());
        for (name, label, detail) in &report.trace {
            println!("  {name:<18} {label}  {detail}");
        }
        return Ok(());
    }

    let start_hidden = cfg.start_hidden;
    let shared = Arc::new(Shared::new(cfg));
    let quit_flag = Arc::new(AtomicBool::new(false));

    let mut hotkeys = HotkeyService::new().map_err(|e| {
        eframe::Error::AppCreation(Box::<dyn std::error::Error + Send + Sync>::from(e.to_string()))
    })?;
    // 启动时就把配置里的热键挂上；失败不阻断启动，UI 会显示红色状态让用户改。
    let spec = shared.snapshot_cfg().hotkey;
    match hotkeys.apply(&spec) {
        Ok(()) => log::info!("热键 {spec} 已注册，后台监听中"),
        Err(e) => log::error!("{e}"),
    }

    spawn_worker(shared.clone());

    let native_options = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default()
            .with_inner_size([600.0, 700.0])
            .with_min_inner_size([480.0, 420.0])
            .with_title("quietkey")
            // 不显式设置的话，eframe 会塞它自带的 egui logo 当窗口图标，
            // 和托盘/exe 的图标对不上。这里统一到 src/icon.rs 那一份。
            .with_icon(window_icon())
            .with_visible(!start_hidden),
        ..Default::default()
    };

    eframe::run_native(
        "quietkey",
        native_options,
        Box::new(move |cc| Ok(Box::new(app::App::new(cc, shared, hotkeys, quit_flag)))),
    )
}

/// 窗口/任务栏图标。取 64px：Windows 会自己往下缩到 16px 的小图标，
/// 反过来放大则会糊。
fn window_icon() -> eframe::egui::IconData {
    const S: u32 = 64;
    eframe::egui::IconData {
        rgba: icon::rgba(S),
        width: S,
        height: S,
    }
}

/// 热键工作线程。
///
/// 刻意不走 UI 线程：窗口查找、`SendMessageTimeout` 这些都可能阻塞上百毫秒，
/// 放在 egui 的 update 里会直接卡住界面；更要紧的是主窗口平时是隐藏的，
/// 根本没有稳定的重绘节奏可依赖。独立线程阻塞在 receiver 上，
/// 没按键时彻底休眠，按下的瞬间被唤醒。
fn spawn_worker(shared: Arc<Shared>) {
    std::thread::Builder::new()
        .name("hotkey-worker".into())
        .spawn(move || {
            // GSMTC 是 WinRT API，调用前必须让本线程处于多线程套间。
            // CoIncrementMTAUsage 会把进程级的 MTA 一直保持住，
            // 比 CoInitializeEx 更省心（不需要配对的 CoUninitialize）。
            if let Err(e) = unsafe { CoIncrementMTAUsage() } {
                log::error!("COM MTA 初始化失败，媒体会话策略将不可用: {e}");
            }

            let rx = GlobalHotKeyEvent::receiver();
            while let Ok(ev) = rx.recv() {
                if ev.state != HotKeyState::Pressed {
                    continue;
                }
                let cfg = shared.snapshot_cfg();
                let report = strategy::toggle_play_pause(&cfg);
                log::info!("热键触发: {}", report.summary());
                shared.record(report);
            }
        })
        .expect("无法启动热键工作线程");
}
