//! 画面を組む部品。ここに見た目を集めて、main.rs は組み立てだけにする。

use crate::style::{self, ACCENT};
use egui::{Align, Button, Color32, CornerRadius, Frame, Layout, RichText, Stroke, Ui, Vec2};

/// 中身を載せる面。地より明るく（暗いテーマでは明るく）して浮かせる
pub fn card(ui: &Ui, accent: bool) -> Frame {
    let s = style::surface(ui);
    Frame::new()
        .fill(if accent { ACCENT.gamma_multiply(0.10) } else { s.fill })
        .stroke(Stroke::new(1.0, if accent { ACCENT } else { s.line }))
        .corner_radius(CornerRadius::same(12))
        .inner_margin(style::GAP_L)
}

/// 横並びの切り替え。選んだ区画だけ差し色で塗る
pub fn segmented<T: PartialEq + Copy>(ui: &mut Ui, cur: &mut T, items: &[(T, &str)]) {
    Frame::new()
        .fill(ui.visuals().widgets.inactive.weak_bg_fill)
        .corner_radius(CornerRadius::same(9))
        .inner_margin(3)
        .show(ui, |ui| {
            ui.spacing_mut().item_spacing.x = 2.0;
            ui.spacing_mut().button_padding = Vec2::new(12.0, 5.0);
            ui.horizontal(|ui| {
                for (v, label) in items {
                    let on = *cur == *v;
                    let text = RichText::new(*label).size(13.5).color(if on {
                        Color32::WHITE
                    } else {
                        ui.visuals().text_color()
                    });
                    let b = Button::new(text)
                        .fill(if on { ACCENT } else { Color32::TRANSPARENT })
                        .stroke(Stroke::NONE)
                        .corner_radius(CornerRadius::same(7));
                    if ui.add(b).clicked() {
                        *cur = *v;
                    }
                }
            });
        });
}

/// 設定の 1 行。左に名前、右に操作部
pub fn row<R>(ui: &mut Ui, label: &str, add: impl FnOnce(&mut Ui) -> R) {
    ui.horizontal(|ui| {
        ui.set_min_height(40.0);
        ui.label(label);
        ui.with_layout(Layout::right_to_left(Align::Center), add);
    });
}

/// 行の間の仕切り
pub fn divider(ui: &mut Ui) {
    let s = style::surface(ui);
    let w = ui.available_width();
    let (rect, _) = ui.allocate_exact_size(Vec2::new(w, 1.0), egui::Sense::hover());
    ui.painter().hline(rect.x_range(), rect.center().y, Stroke::new(1.0, s.divider));
}

/// ドロップ先を示す印。丸の中に下向きの矢印を引く
pub fn drop_mark(ui: &mut Ui, accent: bool) {
    let d = 56.0;
    let (rect, _) = ui.allocate_exact_size(Vec2::splat(d), egui::Sense::hover());
    let c = rect.center();
    let tint = if accent { ACCENT } else { ui.visuals().weak_text_color() };
    ui.painter().circle_filled(c, d * 0.5, tint.gamma_multiply(0.12));
    let stroke = Stroke::new(2.0, tint);
    // 軸
    ui.painter().line_segment([c + Vec2::new(0.0, -12.0), c + Vec2::new(0.0, 8.0)], stroke);
    // 矢
    ui.painter().line_segment([c + Vec2::new(-7.0, 1.0), c + Vec2::new(0.0, 8.0)], stroke);
    ui.painter().line_segment([c + Vec2::new(7.0, 1.0), c + Vec2::new(0.0, 8.0)], stroke);
}

/// 主ボタン。自前で描く。
/// 既定の Button だと幅を広げたときに文字が左に寄り、
/// 押せないときの色が地と同化してボタンごと見えなくなる
pub fn primary(ui: &mut Ui, label: String, enabled: bool) -> egui::Response {
    let size = Vec2::new(ui.available_width(), 46.0);
    let sense = if enabled { egui::Sense::click() } else { egui::Sense::hover() };
    let (rect, resp) = ui.allocate_exact_size(size, sense);

    let fill = if !enabled {
        ACCENT.gamma_multiply(0.35)
    } else if resp.is_pointer_button_down_on() {
        ACCENT.gamma_multiply(0.85)
    } else if resp.hovered() {
        ACCENT.gamma_multiply(1.15)
    } else {
        ACCENT
    };
    ui.painter().rect_filled(rect, CornerRadius::same(10), fill);
    ui.painter().text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        label,
        egui::FontId::proportional(15.0),
        if enabled { Color32::WHITE } else { Color32::WHITE.gamma_multiply(0.85) },
    );
    if enabled && resp.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    resp
}
