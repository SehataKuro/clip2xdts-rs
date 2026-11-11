//! clip2xdts-gui — .clip を投げ込んでタイムシートに変換するだけの GUI

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod font;

use clip2xdts_core::{clip, collect_clips, run_batch, Format, Job};
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

/// 画面のラジオ用。`Format` に Default が無いので薄く包む
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
            && let Err(e) = std::fs::create_dir_all(d) {
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
        let busy = self.running.is_some();

        {
            ui.add_space(4.0);
            ui.heading("clip2xdts");
            ui.label(".clip を落とすと、タイムシートを書き出します");
            ui.add_space(8.0);

            // ドロップ領域
            let zone = egui::Frame::new()
                .fill(if hovering {
                    ui.visuals().selection.bg_fill.linear_multiply(0.35)
                } else {
                    ui.visuals().faint_bg_color
                })
                .stroke(ui.visuals().widgets.noninteractive.bg_stroke)
                .corner_radius(6.0)
                .inner_margin(16.0);
            zone.show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.vertical_centered(|ui| {
                    if self.files.is_empty() {
                        ui.label("ここに .clip ファイルかフォルダをドロップ");
                    } else {
                        ui.label(format!("{} 件", self.files.len()));
                    }
                    ui.add_space(6.0);
                    ui.horizontal(|ui| {
                        if ui.add_enabled(!busy, egui::Button::new("ファイルを選ぶ")).clicked()
                            && let Some(ps) = rfd::FileDialog::new()
                                .add_filter("CLIP STUDIO PAINT", &["clip"])
                                .pick_files()
                            {
                                self.add(ps);
                            }
                        if ui.add_enabled(!busy, egui::Button::new("フォルダを選ぶ")).clicked()
                            && let Some(d) = rfd::FileDialog::new().pick_folder() {
                                self.add([d]);
                            }
                        let can_clear = !busy && !self.files.is_empty();
                        if ui.add_enabled(can_clear, egui::Button::new("消す")).clicked() {
                            self.files.clear();
                        }
                    });
                });
            });

            // 待ち行列
            if !self.files.is_empty() {
                ui.add_space(6.0);
                egui::ScrollArea::vertical().max_height(110.0).id_salt("queue").show(ui, |ui| {
                    for f in &self.files {
                        ui.label(egui::RichText::new(name_of(f)).monospace().small());
                    }
                });
            }

            ui.add_space(10.0);
            ui.separator();
            ui.add_space(6.0);

            // 設定
            ui.horizontal(|ui| {
                ui.label("形式");
                ui.selectable_value(&mut self.format, FormatChoice::Xdts, ".xdts");
                ui.selectable_value(&mut self.format, FormatChoice::Tdts, ".tdts（東映）");
                ui.selectable_value(&mut self.format, FormatChoice::Both, "両方");
            });
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                ui.label("出力先");
                let shown = match &self.out_dir {
                    Some(d) => d.display().to_string(),
                    None => ".clip と同じ場所".to_string(),
                };
                if ui.button(shown).clicked()
                    && let Some(d) = rfd::FileDialog::new().pick_folder() {
                        self.out_dir = Some(d);
                    }
                if self.out_dir.is_some() && ui.button("戻す").clicked() {
                    self.out_dir = None;
                }
            });
            ui.add_space(4.0);
            ui.checkbox(&mut self.all_timelines, "全タイムラインを書き出す");

            ui.add_space(10.0);

            // 実行
            if let Some(r) = &self.running {
                let n = r.done.load(Ordering::SeqCst);
                ui.add(
                    egui::ProgressBar::new(n as f32 / r.total.max(1) as f32)
                        .text(format!("{} / {}", n, r.total)),
                );
            } else {
                let can = !self.files.is_empty();
                let button = egui::Button::new("変換").min_size(egui::vec2(120.0, 30.0));
                if ui.add_enabled(can, button).clicked() {
                    self.start();
                }
            }

            // 結果
            if !self.log.is_empty() {
                ui.add_space(8.0);
                let ng = self.log.iter().filter(|l| !l.ok).count();
                ui.label(format!("成功 {} / 失敗 {}", self.log.len() - ng, ng));
                ui.add_space(4.0);
                egui::ScrollArea::vertical().id_salt("log").show(ui, |ui| {
                    for l in &self.log {
                        let color = if l.ok {
                            ui.visuals().text_color()
                        } else {
                            ui.visuals().error_fg_color
                        };
                        ui.label(egui::RichText::new(&l.text).small().color(color));
                    }
                });
            }
        }
    }
}

fn main() -> eframe::Result {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([560.0, 620.0])
            .with_min_inner_size([420.0, 420.0])
            .with_drag_and_drop(true),
        ..Default::default()
    };
    eframe::run_native(
        "clip2xdts",
        options,
        Box::new(|cc| {
            font::install_japanese(&cc.egui_ctx);
            Ok(Box::<App>::default())
        }),
    )
}
