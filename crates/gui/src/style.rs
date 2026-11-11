//! 画面の調子。文字の大きさ・余白・角丸・面の色をまとめて決める。
//!
//! 地（panel_fill）とカード（surface）の明度を分けるのが肝。
//! 同じ色だと全部が平らに見えて、輪郭線だけの安い画面になる。

use egui::{Color32, CornerRadius, FontFamily, FontId, Stroke, TextStyle, Vec2};

/// 差し色。主ボタンと選択中の区画に使う
pub const ACCENT: Color32 = Color32::from_rgb(0x2f, 0x6f, 0xed);

/// 余白の刻み。これ以外の数字を使わない
pub const GAP_S: f32 = 6.0;
pub const GAP_M: f32 = 12.0;
pub const GAP_L: f32 = 18.0;

/// テーマごとの面の色
pub struct Surface {
    /// カードの地
    pub fill: Color32,
    /// カードの輪郭
    pub line: Color32,
    /// 仕切り線
    pub divider: Color32,
}

pub fn surface(ui: &egui::Ui) -> Surface {
    if ui.visuals().dark_mode {
        Surface {
            fill: Color32::from_rgb(0x1d, 0x1f, 0x23),
            line: Color32::from_rgb(0x2e, 0x31, 0x36),
            divider: Color32::from_rgb(0x2a, 0x2d, 0x31),
        }
    } else {
        Surface {
            fill: Color32::WHITE,
            line: Color32::from_rgb(0xe2, 0xe5, 0xea),
            divider: Color32::from_rgb(0xed, 0xef, 0xf2),
        }
    }
}

pub fn apply(ctx: &egui::Context) {
    ctx.all_styles_mut(tune);
}

/// 明るいテーマ・暗いテーマの両方に同じ調整をかける
fn tune(style: &mut egui::Style) {
    // 文字。本文と注記の差をはっきりさせる
    let p = FontFamily::Proportional;
    style.text_styles = [
        (TextStyle::Heading, FontId::new(21.0, p.clone())),
        (TextStyle::Body, FontId::new(14.5, p.clone())),
        (TextStyle::Button, FontId::new(14.0, p.clone())),
        (TextStyle::Small, FontId::new(12.5, p.clone())),
        (TextStyle::Monospace, FontId::new(12.5, FontFamily::Monospace)),
    ]
    .into();

    // 余白
    style.spacing.item_spacing = Vec2::new(8.0, 8.0);
    style.spacing.button_padding = Vec2::new(12.0, 7.0);
    style.spacing.interact_size.y = 32.0;
    style.spacing.icon_width = 18.0;
    style.spacing.icon_width_inner = 10.0;

    // 角丸
    for w in [
        &mut style.visuals.widgets.noninteractive,
        &mut style.visuals.widgets.inactive,
        &mut style.visuals.widgets.hovered,
        &mut style.visuals.widgets.active,
        &mut style.visuals.widgets.open,
    ] {
        w.corner_radius = CornerRadius::same(7);
    }

    // 地の色をカードより一段沈める。これでカードが浮いて見える
    if style.visuals.dark_mode {
        style.visuals.panel_fill = Color32::from_rgb(0x14, 0x16, 0x19);
        style.visuals.widgets.inactive.weak_bg_fill = Color32::from_rgb(0x2a, 0x2d, 0x33);
        style.visuals.widgets.hovered.weak_bg_fill = Color32::from_rgb(0x33, 0x37, 0x3e);
        style.visuals.widgets.active.weak_bg_fill = Color32::from_rgb(0x3b, 0x40, 0x48);
    } else {
        style.visuals.panel_fill = Color32::from_rgb(0xf4, 0xf5, 0xf7);
        style.visuals.widgets.inactive.weak_bg_fill = Color32::from_rgb(0xf0, 0xf1, 0xf4);
        style.visuals.widgets.hovered.weak_bg_fill = Color32::from_rgb(0xe7, 0xe9, 0xed);
        style.visuals.widgets.active.weak_bg_fill = Color32::from_rgb(0xdd, 0xe0, 0xe5);
    }

    // 枠線は出しすぎない。面の差で区切る
    style.visuals.widgets.inactive.bg_stroke = Stroke::NONE;
    style.visuals.widgets.hovered.bg_stroke = Stroke::NONE;
    style.visuals.widgets.active.bg_stroke = Stroke::NONE;

    style.visuals.selection.bg_fill = ACCENT;
    style.visuals.selection.stroke = Stroke::new(1.0, Color32::WHITE);
    style.visuals.window_corner_radius = CornerRadius::same(10);
}
