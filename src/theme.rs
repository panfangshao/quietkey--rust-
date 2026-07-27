//! Win7 Aero 风格的浅色主题。
//!
//! egui 默认是深灰配色，和这个程序"系统小工具"的定位不搭。这里把配色调成
//! Windows 7 控制面板/对话框那一套：浅灰面板、白底输入框、细边框、
//! 小圆角、天蓝色高亮。
//!
//! **只管窗口内部**。标题栏和窗口边框是 Windows 自己画的，
//! 除非关掉系统装饰自绘（要自己实现拖拽、双击最大化、贴边、缩放，容易出毛病），
//! 否则改不了——这是刻意的取舍。
//!
//! 颜色取自 Win7 的 Aero 视觉样式，集中放在这里，
//! 不散落到各处 `RichText::color(...)` 里，换风格时只改这一个文件。

use eframe::egui::{
    self, Color32, CornerRadius, FontId, Shadow, Stroke, TextStyle, Visuals,
};

/// 对话框/面板底色。
const FACE: Color32 = Color32::from_rgb(0xF0, 0xF0, 0xF0);
/// 输入框、列表的白底。
const FIELD: Color32 = Color32::from_rgb(0xFF, 0xFF, 0xFF);
/// 条纹行的淡底。
const FAINT: Color32 = Color32::from_rgb(0xF7, 0xF7, 0xF7);
/// 按钮静止态底色。
const BUTTON: Color32 = Color32::from_rgb(0xED, 0xED, 0xED);
/// 鼠标悬停时的浅蓝。
const HOVER: Color32 = Color32::from_rgb(0xE5, 0xF3, 0xFB);
/// 按下时的蓝。
const PRESSED: Color32 = Color32::from_rgb(0xCD, 0xE6, 0xF7);
/// 控件边框。
const BORDER: Color32 = Color32::from_rgb(0xAB, 0xAD, 0xB3);
/// 悬停/聚焦时的蓝边框。
const BORDER_HOT: Color32 = Color32::from_rgb(0x3C, 0x7F, 0xB1);
/// 按下时的深蓝边框。
const BORDER_PRESSED: Color32 = Color32::from_rgb(0x2C, 0x62, 0x8B);
/// 分组线。
const SEPARATOR: Color32 = Color32::from_rgb(0xD5, 0xD5, 0xD5);
/// 选中背景。
const SELECT: Color32 = Color32::from_rgb(0x33, 0x99, 0xFF);

/// 正文黑。
pub const TEXT: Color32 = Color32::from_rgb(0x00, 0x00, 0x00);
/// 次要说明文字的灰。
pub const TEXT_WEAK: Color32 = Color32::from_rgb(0x6D, 0x6D, 0x6D);
/// 小节标题的蓝——Win7 任务对话框里的标题就是这个色。
pub const HEADING: Color32 = Color32::from_rgb(0x00, 0x33, 0x99);

// 状态色。浅底上必须比深色主题里那几个更暗，否则看不清。
/// 成功/正常。
pub const OK: Color32 = Color32::from_rgb(0x1E, 0x7A, 0x32);
/// 警告/会打扰用户。
pub const WARN: Color32 = Color32::from_rgb(0xA8, 0x5C, 0x00);
/// 错误。
pub const ERROR: Color32 = Color32::from_rgb(0xB2, 0x22, 0x22);

const RADIUS: u8 = 3;

pub fn install(ctx: &egui::Context) {
    // egui 0.35 给亮色和暗色各存了一套 Style，并按系统主题自动切换。
    // 这个程序只有一套配色，所以两套都写成同样的内容，再把主题**钉死在亮色**——
    // 否则用户的 Windows 切到深色模式时会切走另一套 Style，配色就散了。
    ctx.all_styles_mut(|style| {
        style.visuals = visuals();

        // Win7 对话框比 egui 默认更松一点，按钮也更宽（标准按钮 75×23 逻辑像素）。
        style.spacing.item_spacing = egui::vec2(8.0, 6.0);
        style.spacing.button_padding = egui::vec2(9.0, 4.0);
        style.spacing.indent = 18.0;

        // 对齐 Win7 的系统 UI 字号（9pt ≈ 12px），中文再往上抬半档保证清晰。
        for (text_style, size) in [
            (TextStyle::Heading, 16.0),
            (TextStyle::Body, 13.0),
            (TextStyle::Button, 13.0),
            (TextStyle::Small, 11.0),
        ] {
            if let Some(f) = style.text_styles.get_mut(&text_style) {
                *f = FontId::new(size, f.family.clone());
            }
        }
        style.text_styles.insert(
            TextStyle::Monospace,
            FontId::new(12.5, egui::FontFamily::Monospace),
        );
    });
    ctx.set_theme(egui::ThemePreference::Light);
}

fn visuals() -> Visuals {
    let mut v = Visuals::light();

    v.panel_fill = FACE;
    v.window_fill = FACE;
    v.window_stroke = Stroke::new(1.0, BORDER);
    v.window_corner_radius = CornerRadius::same(RADIUS);
    v.menu_corner_radius = CornerRadius::same(RADIUS);
    v.extreme_bg_color = FIELD;
    v.faint_bg_color = FAINT;
    v.code_bg_color = FIELD;
    v.hyperlink_color = Color32::from_rgb(0x00, 0x66, 0xCC);
    v.warn_fg_color = WARN;
    v.error_fg_color = ERROR;
    v.weak_text_color = Some(TEXT_WEAK);
    v.selection.bg_fill = SELECT;
    v.selection.stroke = Stroke::new(1.0, Color32::WHITE);
    v.striped = true;

    // Win7 的阴影很淡，egui 默认那个太重，会显得"浮"。
    v.window_shadow = Shadow {
        offset: [0, 2],
        blur: 8,
        spread: 0,
        color: Color32::from_black_alpha(40),
    };
    v.popup_shadow = v.window_shadow;

    let w = &mut v.widgets;
    // 非交互元素：标签、分隔线、分组框。
    w.noninteractive.bg_fill = FACE;
    w.noninteractive.weak_bg_fill = FACE;
    w.noninteractive.bg_stroke = Stroke::new(1.0, SEPARATOR);
    w.noninteractive.fg_stroke = Stroke::new(1.0, TEXT);
    w.noninteractive.corner_radius = CornerRadius::same(RADIUS);

    // 静止的按钮/复选框。
    w.inactive.bg_fill = BUTTON;
    w.inactive.weak_bg_fill = BUTTON;
    w.inactive.bg_stroke = Stroke::new(1.0, BORDER);
    w.inactive.fg_stroke = Stroke::new(1.0, TEXT);
    w.inactive.corner_radius = CornerRadius::same(RADIUS);

    // 悬停：浅蓝底 + 蓝边，这是 Aero 最标志性的反馈。
    w.hovered.bg_fill = HOVER;
    w.hovered.weak_bg_fill = HOVER;
    w.hovered.bg_stroke = Stroke::new(1.0, BORDER_HOT);
    w.hovered.fg_stroke = Stroke::new(1.0, TEXT);
    w.hovered.corner_radius = CornerRadius::same(RADIUS);

    // 按下。
    w.active.bg_fill = PRESSED;
    w.active.weak_bg_fill = PRESSED;
    w.active.bg_stroke = Stroke::new(1.0, BORDER_PRESSED);
    w.active.fg_stroke = Stroke::new(1.0, TEXT);
    w.active.corner_radius = CornerRadius::same(RADIUS);

    // 展开的下拉/折叠区。
    w.open.bg_fill = FIELD;
    w.open.weak_bg_fill = BUTTON;
    w.open.bg_stroke = Stroke::new(1.0, BORDER_HOT);
    w.open.fg_stroke = Stroke::new(1.0, TEXT);
    w.open.corner_radius = CornerRadius::same(RADIUS);

    v
}
