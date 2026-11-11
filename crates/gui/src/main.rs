//! clip2xdts-gui — .clip を投げ込んでタイムシートに変換するだけの GUI

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod font;
mod style;
mod widgets;

use clip2xdts_core::{clip, collect_clips, run_batch, Format, Job};
use egui::{Margin, RichText};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::{channel, Receiver};
use std::sync::Arc;

/// 変換の結果 1 行ぶん
struct Line {
    ok: bool,
    text: String,
}

/// 走っている変換
struct Running {
    rx: Receiver<Vec<Line>>,
    done: Arc<AtomicUsize>,
    total: usize,
}

/// 画面のタブ用。`Format` に Default が無いので薄く包む
#[derive(Clone, Copy, PartialEq, Eq, Default)]
enum FormatChoice {
    #[default]
    Xdts,
    Tdts,
    Both,
}

impl From<FormatChoice> for Format {
    fn from(c: FormatChoice) -> Self {
        match c {
            FormatChoice::Xdts => Format::Xdts,
            FormatChoice::Tdts => Format::Tdts,
            FormatChoice::Both => Format::Both,
        }
    }
}

#[derive(Default)]
struct App {
    files: Vec<PathBuf>,
    out_dir: Option<PathBuf>,
    format: FormatChoice,
    all_timelines: bool,
    log: Vec<Line>,
    running: Option<Running>,
}

impl App {
    /// 落とされた／選ばれたパスを待ち行列に足す。重複は捨てる
    fn add(&mut self, paths: impl IntoIterator<Item = PathBuf>) {
        for p in paths {
            let mut found = Vec::new();
            collect_clips(&p, &mut found);
            for f in found {
                let is_clip =
                    f.extension().map(|x| x.eq_ignore_ascii_case("clip")).unwrap_or(false);
                if is_clip && !self.files.contains(&f) {
                    self.files.push(f);
                }
            }
        }
    }

    fn start(&mut self) {
        if self.files.is_empty() || self.running.is_some() {
            return;
        }
        if let Some(d) = &self.out_dir
            && let Err(e) = std::fs::create_dir_all(d)
        {
            self.log.push(Line { ok: false, text: format!("出力フォルダを作れません: {}", e) });
            return;
        }
        let files = self.files.clone();
        let out_dir = self.out_dir.clone();
        let format: Format = self.format.into();
        let all_timelines = self.all_timelines;
        let total = files.len();
        let done = Arc::new(AtomicUsize::new(0));
        let counter = done.clone();
        let (tx, rx) = channel();
        std::thread::spawn(move || {
            let job = Job {
                out_dir: out_dir.as_deref(),
                format,
                options: clip::Options { all_timelines },
                jobs: std::thread::available_parallelism().map(|n| n.get()).unwrap_or(4),
            };
            let results = run_batch(&files, &job, || {
                counter.fetch_add(1, Ordering::SeqCst);
            });
            let lines = files
                .iter()
                .zip(results)
                .flat_map(|(f, r)| match r {
                    Ok(dsts) => dsts
                        .iter()
                        .map(|d| Line { ok: true, text: format!("{} → {}", name_of(f), name_of(d)) })
                        .collect::<Vec<_>>(),
                    Err(e) => vec![Line { ok: false, text: format!("{}: {}", name_of(f), e) }],
                })
                .collect();
            let _ = tx.send(lines);
        });
        self.log.clear();
        self.running = Some(Running { rx, done, total });
    }
}

/// 画面に出す用の短い名前
fn name_of(p: &Path) -> String {
    p.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| p.display().to_string())
}

impl App {
    /// 上の見出し
    fn head(&self, ui: &mut egui::Ui) {
        ui.heading("clip2xdts");
        ui.label(
            RichText::new("CLIP STUDIO PAINT のタイムシートを書き出します")
                .size(13.0)
                .color(ui.visuals().weak_text_color()),
        );
    }

    /// 真ん中。空ならドロップの案内、入っていれば一覧、終わっていれば結果
    fn body(&mut self, ui: &mut egui::Ui, hovering: bool) {
        if !self.log.is_empty() && self.running.is_none() {
            self.results(ui);
            return;
        }
        widgets::card(ui, hovering).show(ui, |ui| {
            ui.set_min_size(ui.available_size());
            if self.files.is_empty() {
                self.invitation(ui, hovering);
            } else {
                self.queue(ui);
            }
        });
    }

    /// 何も入っていないときの案内
    fn invitation(&mut self, ui: &mut egui::Ui, hovering: bool) {
        ui.vertical_centered(|ui| {
            ui.add_space((ui.available_height() - 190.0).max(0.0) * 0.5);
            widgets::drop_mark(ui, hovering);
            ui.add_space(style::GAP_M);
            ui.label(RichText::new("ここに .clip をドロップ").size(16.5).strong());
            ui.add_space(2.0);
            ui.label(
                RichText::new("フォルダごとでも、サブフォルダまで探します")
                    .size(13.0)
                    .color(ui.visuals().weak_text_color()),
            );
            ui.add_space(style::GAP_L);
            ui.horizontal(|ui| {
                // 2 つのボタンを中央に寄せる
                ui.add_space((ui.available_width() - 232.0).max(0.0) * 0.5);
                if ui.button("ファイルを選ぶ").clicked()
                    && let Some(ps) = rfd::FileDialog::new()
                        .add_filter("CLIP STUDIO PAINT", &["clip"])
                        .pick_files()
                {
                    self.add(ps);
                }
                if ui.button("フォルダを選ぶ").clicked()
                    && let Some(d) = rfd::FileDialog::new().pick_folder()
                {
                    self.add([d]);
                }
            });
        });
    }

    /// 入っているファイルの一覧
    fn queue(&mut self, ui: &mut egui::Ui) {
        let busy = self.running.is_some();
        ui.horizontal(|ui| {
            ui.label(RichText::new(format!("{} 件", self.files.len())).size(15.0).strong());
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.add_enabled(!busy, egui::Button::new("消す")).clicked() {
                    self.files.clear();
                }
                if ui.add_enabled(!busy, egui::Button::new("足す")).clicked()
                    && let Some(ps) = rfd::FileDialog::new()
                        .add_filter("CLIP STUDIO PAINT", &["clip"])
                        .pick_files()
                {
                    self.add(ps);
                }
            });
        });
        ui.add_space(style::GAP_M);
        egui::ScrollArea::vertical().id_salt("queue").show(ui, |ui| {
            ui.set_width(ui.available_width());
            for f in &self.files {
                ui.label(RichText::new(name_of(f)).size(13.0));
                ui.add_space(2.0);
            }
        });
    }

    /// 変換が終わったあとの一覧
    fn results(&mut self, ui: &mut egui::Ui) {
        let ng = self.log.iter().filter(|l| !l.ok).count();
        ui.horizontal(|ui| {
            ui.label(RichText::new(format!("成功 {}", self.log.len() - ng)).size(15.0).strong());
            if ng > 0 {
                ui.label(
                    RichText::new(format!("失敗 {}", ng))
                        .size(15.0)
                        .strong()
                        .color(ui.visuals().error_fg_color),
                );
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.button("閉じる").clicked() {
                    self.log.clear();
                }
            });
        });
        ui.add_space(style::GAP_S);
        widgets::card(ui, false).show(ui, |ui| {
            ui.set_min_size(ui.available_size());
            egui::ScrollArea::vertical().id_salt("log").show(ui, |ui| {
                ui.set_width(ui.available_width());
                for l in &self.log {
                    let color =
                        if l.ok { ui.visuals().text_color() } else { ui.visuals().error_fg_color };
                    ui.label(RichText::new(&l.text).size(13.0).color(color));
                    ui.add_space(2.0);
                }
            });
        });
    }

    /// 下の設定
    fn settings(&mut self, ui: &mut egui::Ui) {
        widgets::card(ui, false).inner_margin(egui::Margin::symmetric(16, 2)).show(ui, |ui| {
            widgets::row(ui, "形式", |ui| {
                widgets::segmented(
                    ui,
                    &mut self.format,
                    &[
                        (FormatChoice::Both, "両方"),
                        (FormatChoice::Tdts, ".tdts"),
                        (FormatChoice::Xdts, ".xdts"),
                    ],
                );
            });
            widgets::divider(ui);
            widgets::row(ui, "出力先", |ui| {
                if self.out_dir.is_some()
                    && ui.small_button("✕").on_hover_text("既定に戻す").clicked()
                {
                    self.out_dir = None;
                }
                let shown = match &self.out_dir {
                    Some(d) => name_of(d),
                    None => ".clip と同じ場所".to_string(),
                };
                if ui.button(shown).on_hover_text("出力先のフォルダを選ぶ").clicked()
                    && let Some(d) = rfd::FileDialog::new().pick_folder()
                {
                    self.out_dir = Some(d);
                }
            });
            widgets::divider(ui);
            widgets::row(ui, "全タイムライン", |ui| {
                ui.checkbox(&mut self.all_timelines, "").on_hover_text(
                    "クリスタのタイムラインを複数持つファイルで、選択中の 1 本ではなく
                     全部を書き出します。2 本以上あるときは a_sheet1 / a_sheet2 … に分かれます",
                );
            });
        });
    }

    /// 一番下の「変換」。設定より先に置いて、窓が低くても切れないようにする
    fn action(&mut self, ui: &mut egui::Ui) {
        if let Some(r) = &self.running {
            let n = r.done.load(Ordering::SeqCst);
            ui.add(
                egui::ProgressBar::new(n as f32 / r.total.max(1) as f32)
                    .desired_height(46.0)
                    .corner_radius(9.0)
                    .fill(style::ACCENT)
                    .text(RichText::new(format!("変換中  {} / {}", n, r.total)).strong()),
            );
        } else {
            let can = !self.files.is_empty();
            let label =
                if can { format!("{} 件を変換", self.files.len()) } else { "変換".to_string() };
            if widgets::primary(ui, label, can).clicked() {
                self.start();
            }
        }
    }
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        // ドロップされたファイルを受ける
        let dropped: Vec<PathBuf> =
            ctx.input(|i| i.raw.dropped_files.iter().filter_map(|f| f.path.clone()).collect());
        if !dropped.is_empty() {
            self.add(dropped);
        }
        let hovering = ctx.input(|i| !i.raw.hovered_files.is_empty());

        // 走っている変換の終了を拾う
        if let Some(r) = &self.running {
            if let Ok(lines) = r.rx.try_recv() {
                self.log = lines;
                self.running = None;
            } else {
                ctx.request_repaint_after(std::time::Duration::from_millis(100));
            }
        }

        // 窓全体をテーマの地の色で塗る。塗らないと eframe の既定の消去色
        // （テーマと無関係の暗い半透明）が覗いて、文字色と噛み合わなくなる
        ui.painter().rect_filled(ui.max_rect(), 0.0, ui.visuals().panel_fill);

        let side = Margin { left: 18, right: 18, top: 0, bottom: 0 };
        egui::Panel::top("head")
            .frame(egui::Frame::new().inner_margin(Margin { top: 14, bottom: 10, ..side }))
            .show_separator_line(false)
            .show(ui, |ui| self.head(ui));
        // 「変換」を先に置くことで、窓が低くても最後まで残る
        egui::Panel::bottom("action")
            .frame(egui::Frame::new().inner_margin(Margin { top: 0, bottom: 16, ..side }))
            .show_separator_line(false)
            .show(ui, |ui| self.action(ui));
        egui::Panel::bottom("settings")
            .frame(egui::Frame::new().inner_margin(Margin { top: 12, bottom: 12, ..side }))
            .show_separator_line(false)
            .show(ui, |ui| self.settings(ui));
        egui::CentralPanel::default()
            .frame(egui::Frame::new().inner_margin(side))
            .show(ui, |ui| self.body(ui, hovering));
    }

    /// 窓の消去色。既定はテーマと無関係の暗い半透明なので、地の色に合わせる
    fn clear_color(&self, visuals: &egui::Visuals) -> [f32; 4] {
        visuals.panel_fill.to_normalized_gamma_f32()
    }
}

fn main() -> eframe::Result {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([580.0, 680.0])
            .with_min_inner_size([460.0, 480.0])
            .with_drag_and_drop(true),
        ..Default::default()
    };
    eframe::run_native(
        "clip2xdts",
        options,
        Box::new(|cc| {
            font::install_japanese(&cc.egui_ctx);
            style::apply(&cc.egui_ctx);
            Ok(Box::<App>::default())
        }),
    )
}
