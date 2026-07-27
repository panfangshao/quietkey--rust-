//! egui 设置界面。平时不显示，双击托盘图标才出来。

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use eframe::egui::{self, RichText};
use tray_icon::TrayIconEvent;

use crate::config::Config;
use crate::hotkey::{self, HotkeyService};
use crate::state::Shared;
use crate::strategy;
use crate::theme;
use crate::tray::{self, Tray};
use crate::win::{list_windows, WindowInfo};

pub struct App {
    shared: Arc<Shared>,
    hotkeys: HotkeyService,
    tray: Option<Tray>,
    quit_flag: Arc<AtomicBool>,

    /// 界面上正在编辑的副本，点「保存并应用」才写回 [`Shared`]。
    draft: Config,
    hotkey_error: Option<String>,
    save_notice: Option<(String, bool)>,

    /// 正在等待用户按下新组合键。
    capturing: bool,

    picker_open: bool,
    picker_filter: String,
    picker_rows: Vec<WindowInfo>,
    picker_match_title: bool,

    sessions: Vec<strategy::gsmtc::SessionInfo>,
    /// 点过「刷新媒体会话」没有。用来区分"还没查过"和"查了确实一个都没有"。
    sessions_refreshed: bool,
}

impl App {
    pub fn new(
        cc: &eframe::CreationContext<'_>,
        shared: Arc<Shared>,
        hotkeys: HotkeyService,
        quit_flag: Arc<AtomicBool>,
    ) -> Self {
        install_cjk_font(&cc.egui_ctx);
        theme::install(&cc.egui_ctx);

        tray::wire_wakeup(&cc.egui_ctx);
        let tray = match tray::build("quietkey — 后台播放/暂停") {
            Ok(t) => Some(t),
            Err(e) => {
                log::error!("托盘图标创建失败: {e}");
                None
            }
        };

        let draft = shared.snapshot_cfg();
        Self {
            shared,
            hotkeys,
            tray,
            quit_flag,
            draft,
            hotkey_error: None,
            save_notice: None,
            capturing: false,
            picker_open: false,
            picker_filter: String::new(),
            picker_rows: Vec::new(),
            picker_match_title: false,
            sessions: Vec::new(),
            sessions_refreshed: false,
        }
    }

    fn apply(&mut self) {
        match self.hotkeys.apply(&self.draft.hotkey) {
            Ok(()) => self.hotkey_error = None,
            Err(e) => {
                self.hotkey_error = Some(e.to_string());
                self.save_notice = Some((format!("热键未生效：{e}"), false));
                return;
            }
        }
        *self.shared.cfg.lock().unwrap() = self.draft.clone();
        match self.draft.save() {
            Ok(()) => self.save_notice = Some(("已保存并生效".to_string(), true)),
            Err(e) => self.save_notice = Some((format!("配置写入失败：{e}"), false)),
        }
    }

    fn show_window(&self, ctx: &egui::Context) {
        // 先取消最小化：窗口是被用户最小化（而非缩进托盘）的时候，
        // 只发 Visible(true) 它仍旧躺在任务栏里，看着像"没反应"。
        ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(false));
        ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
        ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
    }

    /// 事件从 [`tray::drain_tray`] / [`tray::drain_menu`] 取，**不是**从 tray-icon
    /// 的全局 channel 取——装了 event handler 之后那两个 channel 收不到任何东西。
    fn pump_tray(&mut self, ctx: &egui::Context) {
        let Some(tray) = &self.tray else { return };

        for ev in tray::drain_tray() {
            // 双击图标打开设置，这是托盘小工具的通行约定。
            if matches!(ev, TrayIconEvent::DoubleClick { .. }) {
                self.show_window(ctx);
            }
        }

        for ev in tray::drain_menu() {
            if ev.id == tray.show_id {
                self.show_window(ctx);
            } else if ev.id == tray.toggle_id {
                let cfg = self.shared.snapshot_cfg();
                let report = strategy::toggle_play_pause(&cfg);
                self.shared.record(report);
            } else if ev.id == tray.quit_id {
                self.quit_flag.store(true, Ordering::SeqCst);
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
        }
    }

    /// 录制模式：读走用户按下的第一个带修饰键的组合键。
    fn capture_hotkey(&mut self, ctx: &egui::Context) {
        if !self.capturing {
            return;
        }
        let captured = ctx.input(|i| {
            for ev in &i.events {
                if let egui::Event::Key {
                    key,
                    pressed: true,
                    modifiers,
                    ..
                } = ev
                {
                    if let Some(spec) = hotkey::spec_from_egui(*key, *modifiers) {
                        return Some(spec);
                    }
                }
            }
            None
        });
        if let Some(spec) = captured {
            self.draft.hotkey = spec;
            self.capturing = false;
            self.hotkey_error = None;
        }
    }
}

impl eframe::App for App {
    /// eframe 0.35 起，`logic` 在每次 `ui` 之前调用，**并且窗口隐藏时只要有人
    /// `request_repaint` 也照样调用**。托盘事件正好落在这个场景里——主窗口
    /// 缩进托盘之后不会再绘制，把托盘轮询放在 `ui` 里就永远收不到菜单点击。
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.pump_tray(ctx);
        self.capture_hotkey(ctx);

        // 关窗不退出，缩回托盘。真正退出只走托盘菜单的「退出」。
        if ctx.input(|i| i.viewport().close_requested()) && !self.quit_flag.load(Ordering::SeqCst) {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            ctx.send_viewport_cmd(egui::ViewportCommand::Visible(false));
        }
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();

        // `ui` 给到的是没有边距和背景的根 Ui，套一层 central_panel 才有正常观感。
        egui::Frame::central_panel(ui.style()).show(ui, |ui| {
            // auto_shrink 关掉，让滚动区填满窗口。默认会缩到内容大小，
            // 窗口下半截就露出没绘制的区域。
            egui::ScrollArea::vertical()
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    self.ui_status(ui);
                    ui.add_space(10.0);
                    self.ui_hotkey(ui);
                    ui.add_space(10.0);
                    self.ui_target(ui);
                    ui.add_space(10.0);
                    self.ui_strategies(ui);
                    ui.add_space(10.0);
                    self.ui_last_result(ui);
                    ui.add_space(10.0);
                    self.ui_diagnostics(ui);
                    ui.add_space(14.0);
                    self.ui_footer(ui, &ctx);
                });
        });

        self.ui_picker(&ctx);
    }
}

impl App {
    fn ui_status(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.heading("quietkey");
            ui.label(RichText::new("后台播放/暂停，不切窗口").weak());
        });
        ui.separator();

        match &self.hotkeys.registered {
            Some(k) => {
                ui.label(
                    RichText::new(format!("● 热键 {k} 已注册，正在后台监听"))
                        .color(theme::OK),
                );
            }
            None => {
                ui.label(RichText::new("● 热键未注册").color(theme::ERROR));
            }
        }
        ui.label(
            RichText::new(format!("累计触发 {} 次", self.shared.trigger_count())).weak(),
        );
    }

    fn ui_hotkey(&mut self, ui: &mut egui::Ui) {
        section(ui, "全局热键");
        ui.horizontal(|ui| {
            if self.capturing {
                ui.label(RichText::new("请按下组合键…").color(theme::WARN));
                if ui.button("取消").clicked() {
                    self.capturing = false;
                }
            } else {
                ui.add(
                    egui::TextEdit::singleline(&mut self.draft.hotkey)
                        .desired_width(180.0)
                        .hint_text("Alt+Q"),
                );
                if ui.button("录制").clicked() {
                    self.capturing = true;
                }
            }
        });
        if let Some(err) = &self.hotkey_error {
            ui.label(RichText::new(err).color(theme::ERROR).small());
        }
        ui.label(
            RichText::new("走系统 RegisterHotKey：空闲时零 CPU，且按键会被系统独占，不会漏给正在打字的窗口。")
                .weak()
                .small(),
        );
    }

    fn ui_target(&mut self, ui: &mut egui::Ui) {
        section(ui, "目标窗口");
        ui.label(RichText::new(self.draft.target.describe()).monospace());

        ui.horizontal(|ui| {
            if ui.button("从当前窗口列表选择…").clicked() {
                self.picker_rows = list_windows();
                self.picker_open = true;
            }
            if ui.button("清空").clicked() {
                self.draft.target = Default::default();
            }
        });

        ui.collapsing("手动编辑匹配规则", |ui| {
            egui::Grid::new("target_grid")
                .num_columns(2)
                .spacing([8.0, 6.0])
                .show(ui, |ui| {
                    ui.label("进程名");
                    text_opt(ui, &mut self.draft.target.exe, "chrome.exe");
                    ui.end_row();

                    ui.label("标题包含");
                    text_opt(ui, &mut self.draft.target.title_contains, "留空更稳");
                    ui.end_row();

                    ui.label("窗口类名");
                    text_opt(ui, &mut self.draft.target.class_name, "Chrome_WidgetWin_1");
                    ui.end_row();
                });
        });

        ui.label(
            RichText::new("存的是匹配规则而不是窗口句柄，所以目标程序重启后依然有效。标题会随播放进度变化，建议留空。")
                .weak()
                .small(),
        );
    }

    fn ui_strategies(&mut self, ui: &mut egui::Ui) {
        section(ui, "策略链");
        ui.label(
            RichText::new("按对你的打扰程度从小到大排列，逐层尝试，第一个成功的即生效。")
                .weak()
                .small(),
        );
        ui.add_space(4.0);

        // 逐个直接借用 self.draft.strategies.*，不预先取 `&mut` 长借用：
        // 中间夹了读 self.draft 其它字段的子选项，长借用会被借用检查器拦下。
        ui.checkbox(&mut self.draft.strategies.gsmtc, "1 · Windows 媒体会话（GSMTC）");
        hint(ui, "浏览器网页视频的主力。不切窗口、不抢焦点，窗口最小化也能控制。");

        if self.draft.strategies.gsmtc {
            ui.indent("follow_recent", |ui| {
                ui.checkbox(
                    &mut self.draft.follow_recent_session,
                    "跟随最近播放/暂停过的那个",
                );
                ui.label(
                    RichText::new(
                        "同时开着多个视频时，热键作用在你最近手动播放或暂停过的那一个上，而不是固定在先开的那个。判据是会话自己上报的时间戳，与具体是哪个播放器无关。上面「目标窗口」填了进程名的话仍以配置为准。",
                    )
                    .weak()
                    .small(),
                );
            });
        }

        ui.checkbox(
            &mut self.draft.strategies.appcommand,
            "2 · WM_APPCOMMAND 定向投递",
        );
        hint(ui, "多媒体键走的那条路，VLC / PotPlayer 等原生播放器支持。同样无感。");

        ui.checkbox(
            &mut self.draft.strategies.postmessage,
            "3 · PostMessage 键盘消息",
        );
        hint(ui, "投递空格给目标窗口。对原生播放器有效，对浏览器网页视频无效。");

        ui.checkbox(
            &mut self.draft.strategies.focus_sendinput,
            "4 · 抢焦点 + SendInput（兜底）",
        );
        hint(
            ui,
            "唯一会打断你的一层：屏幕会闪一下。仅在前三层都无效时才需要打开。",
        );

        if self.draft.strategies.focus_sendinput {
            ui.horizontal(|ui| {
                ui.label("焦点切换等待");
                ui.add(
                    egui::Slider::new(&mut self.draft.focus_settle_ms, 0..=200).suffix(" ms"),
                );
            });
        }
    }

    fn ui_last_result(&mut self, ui: &mut egui::Ui) {
        section(ui, "上次触发");
        let last = self.shared.last.lock().unwrap().clone();
        match last {
            None => {
                ui.label(RichText::new("还没有触发过").weak());
            }
            Some(r) => {
                let color = if !r.ok {
                    theme::ERROR
                } else if r.intrusive {
                    theme::WARN
                } else {
                    theme::OK
                };
                ui.label(RichText::new(r.summary()).color(color));
                if r.intrusive {
                    ui.label(
                        RichText::new("⚠ 走到了兜底层，这次切换了焦点。")
                            .color(theme::WARN)
                            .small(),
                    );
                }
                egui::Grid::new("trace_grid")
                    .num_columns(3)
                    .striped(true)
                    .spacing([10.0, 4.0])
                    .show(ui, |ui| {
                        for (name, label, detail) in &r.trace {
                            ui.label(RichText::new(*name).small());
                            ui.label(RichText::new(*label).small().strong());
                            ui.label(RichText::new(detail).small().weak());
                            ui.end_row();
                        }
                    });
            }
        }
    }

    fn ui_diagnostics(&mut self, ui: &mut egui::Ui) {
        section(ui, "诊断");
        ui.horizontal(|ui| {
            if ui.button("立即测试一次").clicked() {
                // 用界面上的草稿测，方便边调边试，不必先保存。
                let report = strategy::toggle_play_pause(&self.draft);
                self.shared.record(report);
            }
            if ui.button("刷新媒体会话").clicked() {
                self.sessions = strategy::gsmtc::list_sessions();
                self.sessions_refreshed = true;
            }
        });
        if !self.sessions.is_empty() {
            ui.add_space(4.0);
            for s in &self.sessions {
                // ★ 标出「最近活跃」，也就是热键当前会跟过去的那个会话——
                // 多播放器场景下这一行就是"为什么控的是它"的答案。
                let activity = if !s.has_timeline {
                    "不上报时间线".to_string()
                } else if s.recent {
                    "最近碰过".to_string()
                } else {
                    format!("{:.1} 秒前碰过", s.behind_secs)
                };
                let line = format!(
                    "{} {} {}  · {}",
                    if s.playing { "▶" } else { "⏸" },
                    if s.recent { "★" } else { "  " },
                    s.aumid,
                    activity
                );
                let text = RichText::new(line).small().monospace();
                ui.label(if s.recent {
                    text.color(theme::OK)
                } else {
                    text
                });
            }
            ui.label(
                RichText::new("★ = 最近被你播放/暂停过的那个，热键当前会作用在它上面")
                    .weak()
                    .small(),
            );
        } else if self.sessions_refreshed {
            // 空列表是重要证据：浏览器没注册媒体会话的话，第 1 层根本够不着它。
            ui.label(
                RichText::new("系统当前没有任何媒体会话。浏览器只有在播放【带声音且时长足够】的媒体时才会注册会话；若某个浏览器始终不出现在这里，第 1 层就控制不了它。")
                    .color(theme::WARN)
                    .small(),
            );
        }

    }

    fn ui_footer(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        ui.separator();
        ui.horizontal(|ui| {
            if ui.button("保存并应用").clicked() {
                self.apply();
            }
            if ui.button("缩到托盘").clicked() {
                ctx.send_viewport_cmd(egui::ViewportCommand::Visible(false));
            }
            if let Some((msg, ok)) = &self.save_notice {
                let c = if *ok {
                    theme::OK
                } else {
                    theme::ERROR
                };
                ui.label(RichText::new(msg).color(c).small());
            }
        });
        ui.checkbox(&mut self.draft.start_hidden, "启动时直接缩到托盘");
    }

    fn ui_picker(&mut self, ctx: &egui::Context) {
        if !self.picker_open {
            return;
        }
        let mut open = true;
        let mut chosen: Option<WindowInfo> = None;

        egui::Window::new("选择目标窗口")
            .open(&mut open)
            .default_size([620.0, 460.0])
            .collapsible(false)
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.label("筛选");
                    ui.add(
                        egui::TextEdit::singleline(&mut self.picker_filter)
                            .desired_width(240.0)
                            .hint_text("进程名或标题"),
                    );
                    if ui.button("刷新").clicked() {
                        self.picker_rows = list_windows();
                    }
                });
                ui.checkbox(
                    &mut self.picker_match_title,
                    "同时把当前标题记为匹配条件（标题会变，一般不勾）",
                );
                ui.separator();

                let f = self.picker_filter.to_lowercase();
                egui::ScrollArea::vertical().show(ui, |ui| {
                    for w in &self.picker_rows {
                        if !f.is_empty()
                            && !w.title.to_lowercase().contains(&f)
                            && !w.exe.to_lowercase().contains(&f)
                        {
                            continue;
                        }
                        let label = format!("{:<22} {}", w.exe, w.title);
                        if ui
                            .selectable_label(false, RichText::new(label).monospace())
                            .on_hover_text(format!("类名 {}   PID {}", w.class_name, w.pid))
                            .clicked()
                        {
                            chosen = Some(w.clone());
                        }
                    }
                });
            });

        if let Some(w) = chosen {
            self.draft.target.exe = Some(w.exe.clone());
            self.draft.target.class_name = Some(w.class_name.clone());
            self.draft.target.title_contains = if self.picker_match_title {
                Some(w.title.clone())
            } else {
                None
            };
            self.picker_open = false;
        } else {
            self.picker_open = open;
        }
    }
}

/// 小节标题。用 Win7 任务对话框那种蓝色标题，让分区一眼看得出来。
fn section(ui: &mut egui::Ui, text: &str) {
    ui.label(RichText::new(text).strong().color(theme::HEADING));
}

fn hint(ui: &mut egui::Ui, text: &str) {
    ui.indent(text, |ui| {
        ui.label(RichText::new(text).weak().small());
    });
}

fn text_opt(ui: &mut egui::Ui, slot: &mut Option<String>, hint: &str) {
    let mut buf = slot.clone().unwrap_or_default();
    ui.add(
        egui::TextEdit::singleline(&mut buf)
            .desired_width(260.0)
            .hint_text(hint),
    );
    *slot = if buf.trim().is_empty() { None } else { Some(buf) };
}

/// 装一个中文字体。
///
/// egui 自带的字体只有拉丁字符，不换字体的话整个界面全是豆腐块。
/// 优先挑纯 TTF——TTC 字体集合在字体后端里的支持不稳定。
fn install_cjk_font(ctx: &egui::Context) {
    const CANDIDATES: &[&str] = &[
        r"C:\Windows\Fonts\Deng.ttf",    // 等线，Win10+ 自带
        r"C:\Windows\Fonts\simhei.ttf",  // 黑体
        r"C:\Windows\Fonts\msyh.ttc",    // 微软雅黑
        r"C:\Windows\Fonts\simsun.ttc",  // 宋体
    ];

    for path in CANDIDATES {
        let Ok(bytes) = std::fs::read(path) else {
            continue;
        };
        let mut fonts = egui::FontDefinitions::default();
        fonts
            .font_data
            .insert("cjk".to_owned(), std::sync::Arc::new(egui::FontData::from_owned(bytes)));
        for family in [egui::FontFamily::Proportional, egui::FontFamily::Monospace] {
            fonts.families.entry(family).or_default().insert(0, "cjk".to_owned());
        }
        ctx.set_fonts(fonts);
        log::info!("已加载中文字体 {path}");
        return;
    }
    log::warn!("没有找到可用的中文字体，界面中文会显示为方块");
}
