//! The time sheet drawn as the common paper sheet: A4 portrait, 6 seconds a
//! page in two columns of 72 frames, ACTION・セリフ・CELL・CAMERA columns and
//! the 話数・TITLE・SCENE・CUT・SECOND・NAME・シート header (3-second sheets,
//! one column a page of A–J, when a cut has more cel columns than five). The printed
//! letters A, B, C … stay; the tracks' names are written on the row below.
//! The ACTION (原画) and CELL (動画) columns are the same width. 原画: keys
//! ringed, 中割参考 in a triangle, 中割指示 ○ ● (the p1 marks), × , a line while
//! held and a wave after ×. 動画: the number of each drawing (in-betweens at
//! ○ ● get numbers of their own), a line while held, × and a wave.
//!
//! Text uses a Japanese font: [`Header::font`] if given, else one that comes
//! with the system (Windows, macOS, Linux with Noto CJK).
//!
//! ```no_run
//! # let sheet = xdts::Sheet::default();
//! let header = clip2xdts_paper::Header { cut: "c013".into(), fps: 24, hold: 6, ..Default::default() };
//! let files = clip2xdts_paper::write_pages(&sheet, &header, std::path::Path::new("out/c013")).unwrap();
//! ```

use ab_glyph::{Font, FontVec, PxScale, ScaleFont, point};
use image::{Rgba, RgbaImage};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use xdts::{NULL_CELL, Sheet, TimeTable, Track};

type Result<T> = std::result::Result<T, String>;

fn safety(message: impl Into<String>) -> String {
    message.into()
}

/// Frames on one page: two columns of 3 seconds at 24 fps.
pub const FRAMES_PER_PAGE: usize = 144;
const HALF: usize = FRAMES_PER_PAGE / 2;

/// What goes in the header.
#[derive(Debug, Clone, Default)]
pub struct Header {
    pub title: String,
    pub episode: String,
    pub scene: String,
    pub cut: String,
    pub fps: u32,
    /// Only the 原画 (ACTION) columns; the 動画 (CELL) ones are left blank.
    pub action_only: bool,
    /// A hold line only for a drawing held this many frames or more (0: always).
    pub hold: usize,
    pub colors: Colors,
    /// A font file (TTF / OTF / TTC) to write with, instead of a system one.
    pub font: Option<PathBuf>,
}

impl Header {
    /// The header a sheet says itself (cut, scene, episode), at 24 fps, hold
    /// lines from 6 frames, green paper.
    pub fn for_sheet(sheet: &Sheet) -> Header {
        Header {
            cut: sheet.cut.clone(),
            scene: sheet.scene.clone(),
            episode: sheet.episode.clone(),
            fps: 24,
            hold: 6,
            ..Header::default()
        }
    }
}

/// Pages a sheet of `duration` frames and `tracks` cel columns takes (see [`layout`]).
pub fn pages(duration: usize, tracks: usize) -> usize {
    layout(duration, tracks).0
}

// A4 at 150 dpi (a 3-second sheet is one column of it wide).
const H: u32 = 1754;
const M: i32 = 40;
/// The paper's colours: the paper, the printed ink, and the pen written with.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Colors {
    pub paper: [u8; 3],
    pub ink: [u8; 3],
    pub pen: [u8; 3],
}

impl Default for Colors {
    /// Green paper, green ink, a black pen.
    fn default() -> Self {
        Colors {
            paper: [198, 236, 184],
            ink: [52, 98, 62],
            pen: [20, 24, 20],
        }
    }
}

/// `#rrggbb`, if it is one.
pub fn hex_color(text: &str) -> Option<[u8; 3]> {
    let hex = text.strip_prefix('#')?;
    if hex.len() != 6 || !hex.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    let at = |i: usize| u8::from_str_radix(&hex[i..i + 2], 16).ok();
    Some([at(0)?, at(2)?, at(4)?])
}

// The two frame columns.
const BLOCK_TOP: i32 = 250;
const BLOCK_W: i32 = 560;
const BLOCK_GAP: i32 = 40;
const HEAD_ROW: i32 = 18;
const ROW: i32 = 19;
const FRAME_COL: i32 = 34;
const SECOND_COL: i32 = 16;
const SPEECH_COL: i32 = 40;
const CAMERA_COL: i32 = 70;

fn font(given: Option<&Path>) -> Result<FontVec> {
    let mut candidates: Vec<PathBuf> = given.map(Path::to_path_buf).into_iter().collect();
    let windows = std::env::var("WINDIR").unwrap_or_else(|_| "C:\\Windows".into());
    for name in [
        "BIZ-UDGothicR.ttc",
        "YuGothM.ttc",
        "meiryo.ttc",
        "msgothic.ttc",
    ] {
        candidates.push(Path::new(&windows).join("Fonts").join(name));
    }
    for path in [
        "/System/Library/Fonts/ヒラギノ角ゴシック W3.ttc",
        "/System/Library/Fonts/Hiragino Sans GB.ttc",
        "/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc",
        "/usr/share/fonts/noto-cjk/NotoSansCJK-Regular.ttc",
    ] {
        candidates.push(PathBuf::from(path));
    }
    for path in candidates {
        if let Ok(bytes) = std::fs::read(&path)
            && let Ok(font) = FontVec::try_from_vec_and_index(bytes, 0)
        {
            return Ok(font);
        }
    }
    Err(safety(
        "紙のタイムシートに使う日本語フォントが見つかりません（Header::font で指定できます）。",
    ))
}

#[derive(Clone, Copy)]
enum Align {
    Left,
    Center,
    Right,
}

struct Page<'a> {
    img: RgbaImage,
    font: &'a FontVec,
    ink: [u8; 3],
    /// The ink at half strength, for the lines between frames and columns.
    faint: [u8; 3],
    pen: [u8; 3],
}

impl Page<'_> {
    fn blend(&mut self, x: i32, y: i32, color: [u8; 3], a: f32) {
        if x < 0
            || y < 0
            || x >= self.img.width() as i32
            || y >= self.img.height() as i32
            || a <= 0.0
        {
            return;
        }
        let p = self.img.get_pixel_mut(x as u32, y as u32);
        let a = a.min(1.0);
        for c in 0..3 {
            p[c] = (p[c] as f32 * (1.0 - a) + color[c] as f32 * a).round() as u8;
        }
    }

    fn fill(&mut self, x0: i32, y0: i32, x1: i32, y1: i32, color: [u8; 3]) {
        for y in y0.max(0)..y1.min(self.img.height() as i32) {
            for x in x0.max(0)..x1.min(self.img.width() as i32) {
                self.blend(x, y, color, 1.0);
            }
        }
    }

    /// A horizontal line `weight` px thick, its top at `y`.
    fn hline(&mut self, x0: i32, x1: i32, y: i32, weight: i32, color: [u8; 3]) {
        self.fill(x0, y, x1, y + weight, color);
    }

    fn vline(&mut self, x: i32, y0: i32, y1: i32, weight: i32, color: [u8; 3]) {
        self.fill(x, y0, x + weight, y1, color);
    }

    /// A wavy vertical line around `x`; its phase follows `y` on the page, so
    /// waves drawn row by row join up.
    fn wave(&mut self, x: i32, y0: i32, y1: i32, color: [u8; 3]) {
        const PERIOD: f32 = 18.0;
        const AMPLITUDE: f32 = 3.0;
        let at =
            |y: i32| (AMPLITUDE * (y as f32 * std::f32::consts::TAU / PERIOD).sin()).round() as i32;
        for y in y0..y1 {
            // Join to the row above so the steep parts have no gaps.
            let (a, b) = (at(y - 1), at(y));
            self.fill(x + a.min(b) - 1, y, x + a.max(b) + 1, y + 1, color);
        }
    }

    fn rect(&mut self, x0: i32, y0: i32, x1: i32, y1: i32, weight: i32, color: [u8; 3]) {
        self.hline(x0, x1, y0, weight, color);
        self.hline(x0, x1, y1 - weight, weight, color);
        self.vline(x0, y0, y1, weight, color);
        self.vline(x1 - weight, y0, y1, weight, color);
    }

    fn width_of(&self, text: &str, px: f32) -> f32 {
        let font = self.font.as_scaled(PxScale::from(px));
        let mut width = 0.0;
        let mut prev = None;
        for c in text.chars() {
            let id = font.glyph_id(c);
            if let Some(p) = prev {
                width += font.kern(p, id);
            }
            width += font.h_advance(id);
            prev = Some(id);
        }
        width
    }

    /// Text centred on `mid` vertically; shrunk to fit `fit` px wide if given.
    #[allow(clippy::too_many_arguments)]
    fn text(
        &mut self,
        text: &str,
        x: f32,
        mid: f32,
        px: f32,
        align: Align,
        color: [u8; 3],
        fit: Option<f32>,
    ) {
        let mut px = px;
        if let Some(fit) = fit {
            let width = self.width_of(text, px);
            if width > fit && width > 0.0 {
                px = (px * fit / width).max(6.0);
            }
        }
        let scale = PxScale::from(px);
        let width = self.width_of(text, px);
        let font = self.font.as_scaled(scale);
        let baseline = mid + (font.ascent() + font.descent()) / 2.0;
        let mut caret = match align {
            Align::Left => x,
            Align::Center => x - width / 2.0,
            Align::Right => x - width,
        };
        let mut prev = None;
        for c in text.chars() {
            let id = font.glyph_id(c);
            if let Some(p) = prev {
                caret += font.kern(p, id);
            }
            let glyph = id.with_scale_and_position(scale, point(caret, baseline));
            if let Some(outline) = self.font.outline_glyph(glyph) {
                let bounds = outline.px_bounds();
                let mut marks = Vec::new();
                outline.draw(|gx, gy, cover| {
                    marks.push((
                        bounds.min.x as i32 + gx as i32,
                        bounds.min.y as i32 + gy as i32,
                        cover,
                    ))
                });
                for (gx, gy, cover) in marks {
                    self.blend(gx, gy, color, cover);
                }
            }
            caret += font.h_advance(id);
            prev = Some(id);
        }
    }
}

/// What a cel column shows where its cel changes.
fn mark(value: &str) -> Option<String> {
    match value {
        NULL_CELL => Some("×".into()),
        "SYMBOL_HYPHEN" | "" => None,
        other => Some(other.to_string()),
    }
}

const TICK_1: &str = "SYMBOL_TICK_1";
const TICK_2: &str = "SYMBOL_TICK_2";
const REFERENCEFRAME: &str = "OPTION_REFERENCEFRAME";

fn is_tick(value: &str) -> bool {
    matches!(value, TICK_1 | TICK_2 | "SYMBOL_HYPHEN")
}

/// The cel (or ×) a track holds at `frame`, ○ ● and `-` keeping the one before.
fn real_at(track: &Track, frame: usize) -> Option<&str> {
    track
        .frames
        .iter()
        .take_while(|(f, _)| *f <= frame)
        .map(|(_, c)| c.value.as_str())
        .filter(|v| !is_tick(v))
        .last()
}

/// Pages, and frames on each: 6-second sheets of A–E, or 3-second sheets of
/// A–J when there are more cel columns than five.
pub fn layout(duration: usize, tracks: usize) -> (usize, usize) {
    if tracks > 5 {
        (duration.div_ceil(HALF).max(1), HALF)
    } else {
        (duration.div_ceil(FRAMES_PER_PAGE).max(1), FRAMES_PER_PAGE)
    }
}

fn draw_header(
    page: &mut Page,
    header: &Header,
    duration: usize,
    fps: usize,
    index: usize,
    count: usize,
) {
    let (top, label, value) = (M, 22, 44);
    let right = page.img.width() as i32 - M;
    // Widths for a 6-second sheet, scaled to the page.
    let scale = (right - M) as f32 / 1160.0;
    let columns: [(&str, f32, String); 7] = [
        ("話数", 90.0, header.episode.clone()),
        ("TITLE", 330.0, header.title.clone()),
        ("SCENE", 90.0, header.scene.clone()),
        ("CUT", 110.0, header.cut.clone()),
        (
            "SECOND",
            190.0,
            format!("{}  +  {}", duration / fps, duration % fps),
        ),
        ("NAME", 170.0, String::new()),
        ("シート", 180.0, String::new()),
    ];
    page.rect(M, top, right, top + label + value, 2, page.ink);
    page.hline(M, right, top + label, 1, page.ink);
    let mut x = M;
    for (i, (name, base, text)) in columns.iter().enumerate() {
        let width = if i == columns.len() - 1 {
            right - x
        } else {
            (base * scale).round() as i32
        };
        if i > 0 {
            page.vline(x, top, top + label + value, 1, page.ink);
        }
        let mid = x as f32 + width as f32 / 2.0;
        page.text(
            name,
            mid,
            (top + label / 2) as f32,
            12.0,
            Align::Center,
            page.ink,
            Some(width as f32 - 4.0),
        );
        if !text.is_empty() {
            page.text(
                text,
                mid,
                (top + label + value / 2) as f32,
                20.0,
                Align::Center,
                page.pen,
                Some(width as f32 - 12.0),
            );
        }
        if *name == "シート" {
            // 「N 枚 ／ n 枚目」 split by a diagonal.
            let (x0, y0, y1) = (x, top + label, top + label + value);
            for t in 0..width {
                page.blend(x0 + t, y1 - (t * value) / width.max(1), page.ink, 1.0);
            }
            page.text(
                &format!("{count} 枚"),
                x0 as f32 + width as f32 * 0.3,
                (y0 + 14) as f32,
                15.0,
                Align::Center,
                page.pen,
                None,
            );
            page.text(
                &format!("{} 枚目", index + 1),
                x0 as f32 + width as f32 * 0.72,
                (y1 - 13) as f32,
                15.0,
                Align::Center,
                page.pen,
                None,
            );
        }
        x += width;
    }
}

/// A ring (原画) or a triangle (中割参考) around a number in the ACTION column.
fn keyed(page: &mut Page, cx: i32, mid: i32, number: &str, triangle: bool) {
    let (fx, fy) = (cx as f32, mid as f32);
    if triangle {
        let corners = [(fx, fy - 9.0), (fx - 9.5, fy + 7.0), (fx + 9.5, fy + 7.0)];
        for k in 0..3 {
            let ((x0, y0), (x1, y1)) = (corners[k], corners[(k + 1) % 3]);
            for s in 0..=40 {
                let t = s as f32 / 40.0;
                page.blend(
                    (x0 + (x1 - x0) * t).round() as i32,
                    (y0 + (y1 - y0) * t).round() as i32,
                    page.pen,
                    1.0,
                );
            }
        }
        page.text(
            number,
            fx,
            fy + 2.0,
            8.0,
            Align::Center,
            page.pen,
            Some(10.0),
        );
    } else {
        ring(page, cx, mid, 8.5, false);
        page.text(number, fx, fy, 9.0, Align::Center, page.pen, Some(13.0));
    }
}

/// A circle outline, or a filled one (●).
fn ring(page: &mut Page, cx: i32, cy: i32, radius: f32, filled: bool) {
    let r = radius.ceil() as i32 + 1;
    for y in -r..=r {
        for x in -r..=r {
            let d = ((x * x + y * y) as f32).sqrt();
            let cover = if filled {
                (radius + 0.5 - d).clamp(0.0, 1.0)
            } else {
                (1.0 - (d - radius).abs()).clamp(0.0, 1.0)
            };
            page.blend(cx + x, cy + y, page.pen, cover);
        }
    }
}

/// Every page of the sheet, in order.
pub fn render(sheet: &Sheet, header: &Header) -> Result<Vec<RgbaImage>> {
    let table = sheet
        .tables
        .first()
        .ok_or_else(|| safety("タイムシートが空です。"))?;
    let font = font(header.font.as_deref())?;
    let (count, per_page) = layout(table.duration, table.columns.len());
    // A 6-second sheet: two blocks of A–E. A 3-second sheet: one block, the
    // page wide, of A–J.
    let (blocks, columns) = if per_page == FRAMES_PER_PAGE {
        (2, 5)
    } else {
        (1, 10)
    };
    let width = 2 * M + 2 * BLOCK_W + BLOCK_GAP;
    let block_w = (width - 2 * M - (blocks - 1) * BLOCK_GAP) / blocks;
    let columns = columns.max(table.columns.len());
    let fps = header.fps.max(1) as usize;
    let douga: Vec<Douga> = table.tracks.iter().map(Douga::of).collect();
    // Of the two 中割 marks, the one used more is drawn ・, the other a small ○.
    let used = |kind: &str| {
        table
            .tracks
            .iter()
            .flat_map(|t| &t.frames)
            .filter(|(_, c)| c.value == kind)
            .count()
    };
    let style = Style {
        dot: if used(TICK_1) >= used(TICK_2) {
            TICK_1
        } else {
            TICK_2
        },
        action_only: header.action_only,
        hold: header.hold,
    };
    let mut out = Vec::with_capacity(count);
    for index in 0..count {
        let c = header.colors;
        let faint = [0, 1, 2].map(|i| ((c.ink[i] as u16 + c.paper[i] as u16) / 2) as u8);
        let paper = Rgba([c.paper[0], c.paper[1], c.paper[2], 255]);
        let mut page = Page {
            img: RgbaImage::from_pixel(width as u32, H, paper),
            font: &font,
            ink: c.ink,
            faint,
            pen: c.pen,
        };
        draw_header(&mut page, header, table.duration, fps, index, count);
        page.text("MEMO", M as f32, 130.0, 13.0, Align::Left, page.ink, None);
        for block in 0..blocks {
            let x = M + block * (block_w + BLOCK_GAP);
            let at = Block {
                x,
                width: block_w,
                columns: columns as i32,
                first: index * per_page + block as usize * HALF,
            };
            draw_block(&mut page, &at, table, &douga, &style, fps);
        }
        out.push(page.img);
    }
    Ok(out)
}

/// How the marks are drawn on every page.
struct Style {
    /// The 中割 mark drawn ・ (the other is a small ○).
    dot: &'static str,
    action_only: bool,
    hold: usize,
}

/// How long the drawing shown at `frame` is held: from the change it starts at
/// to the next one (○ ● count as changes when `ticks`), or the end of the cut.
fn held_for(track: &Track, frame: usize, ticks: bool, duration: usize) -> usize {
    let counts = |v: &str| ticks || !is_tick(v);
    let start = track
        .frames
        .iter()
        .rev()
        .find(|(f, c)| *f <= frame && counts(&c.value))
        .map(|(f, _)| *f)
        .unwrap_or(0);
    let end = track
        .frames
        .iter()
        .find(|(f, c)| *f > frame && counts(&c.value))
        .map(|(f, _)| *f)
        .unwrap_or(duration);
    end.saturating_sub(start)
}

/// Where one 72-frame block goes and how many A, B, C … columns it has.
struct Block {
    x: i32,
    width: i32,
    columns: i32,
    first: usize,
}

/// The 動画 numbers of a track whose 中割 marks call for in-betweens: every
/// drawing numbered in order of appearance (a key again keeps its number,
/// each ○ / ● is a new in-between). `None`: no marks, the numbers as they are.
struct Douga(Option<HashMap<usize, String>>);

impl Douga {
    fn of(track: &Track) -> Douga {
        if !track
            .frames
            .iter()
            .any(|(_, c)| matches!(c.value.as_str(), TICK_1 | TICK_2))
        {
            return Douga(None);
        }
        let mut keys: HashMap<&str, usize> = HashMap::new();
        let mut next = 0;
        let mut at = HashMap::new();
        for (frame, cell) in &track.frames {
            let value = cell.value.as_str();
            let number = match value {
                TICK_1 | TICK_2 => {
                    next += 1;
                    next
                }
                v if v.starts_with("SYMBOL_") || v.is_empty() => continue,
                v => *keys.entry(v).or_insert_with(|| {
                    next += 1;
                    next
                }),
            };
            at.insert(*frame, number.to_string());
        }
        Douga(Some(at))
    }
}

fn draw_block(
    page: &mut Page,
    b: &Block,
    table: &TimeTable,
    douga: &[Douga],
    style: &Style,
    fps: usize,
) {
    let n = b.columns;
    // 原画 (ACTION) and 動画 (CELL) columns are the same width.
    let col = (b.width - FRAME_COL - SECOND_COL - SPEECH_COL - CAMERA_COL) / (2 * n);
    let action_x = b.x;
    let frame_x = action_x + n * col;
    let second_x = frame_x + FRAME_COL;
    let speech_x = second_x + SECOND_COL;
    let cell_x = speech_x + SPEECH_COL;
    let camera_x = cell_x + n * col;
    let right = b.x + b.width;
    let top = BLOCK_TOP;
    let first = b.first;
    // Group names, the printed letters, and the column names (written in).
    let letters_y = top + HEAD_ROW;
    let names_y = top + 2 * HEAD_ROW;
    let grid = top + 3 * HEAD_ROW;
    let bottom = grid + HALF as i32 * ROW;

    page.hline(action_x, frame_x, letters_y, 1, page.ink);
    page.hline(cell_x, camera_x, letters_y, 1, page.ink);
    page.hline(action_x, frame_x, names_y, 1, page.faint);
    page.hline(cell_x, camera_x, names_y, 1, page.faint);
    page.text(
        "ACTION",
        (action_x + frame_x) as f32 / 2.0,
        (top + HEAD_ROW / 2) as f32,
        10.0,
        Align::Center,
        page.ink,
        None,
    );
    page.text(
        "セリフ",
        (second_x + cell_x) as f32 / 2.0,
        (top + HEAD_ROW * 3 / 2) as f32,
        11.0,
        Align::Center,
        page.ink,
        None,
    );
    page.text(
        "CELL",
        (cell_x + camera_x) as f32 / 2.0,
        (top + HEAD_ROW / 2) as f32,
        10.0,
        Align::Center,
        page.ink,
        None,
    );
    page.text(
        "CAMERA",
        (camera_x + right) as f32 / 2.0,
        (top + HEAD_ROW * 3 / 2) as f32,
        10.0,
        Align::Center,
        page.ink,
        None,
    );
    let letter = |i: i32| char::from(b'A' + (i % 26) as u8).to_string();
    for i in 0..n {
        for x0 in [action_x, cell_x] {
            let cx = (x0 + i * col + col / 2) as f32;
            page.text(
                &letter(i),
                cx,
                (letters_y + HEAD_ROW / 2) as f32,
                10.0,
                Align::Center,
                page.ink,
                None,
            );
            if let Some(name) = table.columns.get(i as usize) {
                page.text(
                    name,
                    cx,
                    (names_y + HEAD_ROW / 2) as f32,
                    10.0,
                    Align::Center,
                    page.pen,
                    Some(col as f32 - 4.0),
                );
            }
        }
    }

    // Rows: a faint line each frame, darker each 6, heavy each second.
    for row in 0..=HALF as i32 {
        let frame = first + row as usize; // frames above this line
        let y = grid + row * ROW;
        let (weight, color) = if frame.is_multiple_of(fps) {
            (2, page.ink)
        } else if frame.is_multiple_of(6) {
            (1, page.ink)
        } else {
            (1, page.faint)
        };
        // セリフ (with the seconds strip beside it) has no lines across, but
        // for the one under the heading.
        if row == 0 {
            page.hline(action_x, right, y, weight, color);
        } else {
            page.hline(action_x, second_x, y, weight, color);
            page.hline(cell_x, right, y, weight, color);
        }
    }
    // Columns: faint inside a group, ink between groups, heavy outside.
    for i in 1..n {
        page.vline(action_x + i * col, letters_y, bottom, 1, page.faint);
        page.vline(cell_x + i * col, letters_y, bottom, 1, page.faint);
    }
    // No line between the seconds strip and セリフ: one column.
    for edge in [frame_x, second_x, cell_x, camera_x] {
        page.vline(edge, top, bottom, 1, page.ink);
    }
    page.rect(action_x, top, right, bottom, 2, page.ink);

    // Frame numbers every 2, seconds each second, ▸ each half second.
    for row in 0..HALF {
        let frame = first + row + 1;
        let mid = (grid + row as i32 * ROW + ROW / 2) as f32;
        if frame.is_multiple_of(2) {
            page.text(
                &frame.to_string(),
                (second_x - 3) as f32,
                mid,
                11.0,
                Align::Right,
                page.ink,
                None,
            );
        }
        if frame.is_multiple_of(fps) {
            page.text(
                &(frame / fps).to_string(),
                (second_x + SECOND_COL / 2) as f32,
                mid,
                11.0,
                Align::Center,
                page.ink,
                None,
            );
        } else if frame.is_multiple_of((fps / 2).max(1)) {
            // ▸ right-aligned, its tip on the セリフ｜CELL line, centred on the
            // line between this frame and the next. (Drawn: not every font has it.)
            let ty = grid + (row as i32 + 1) * ROW;
            for dx in 0..6 {
                page.vline(
                    cell_x - 6 + dx,
                    ty - (5 - dx),
                    ty + (5 - dx) + 1,
                    1,
                    page.ink,
                );
            }
        }
    }

    for (i, track) in table.tracks.iter().enumerate() {
        let i = i as i32;
        let ax = action_x + i * col + col / 2;
        let cx = cell_x + i * col + col / 2;
        let numbers = douga.get(i as usize).and_then(|d| d.0.as_ref());
        for row in 0..HALF {
            let frame = first + row;
            if frame >= table.duration {
                break;
            }
            let y = grid + row as i32 * ROW;
            let mid = y + ROW / 2;
            let here = track
                .frames
                .iter()
                .find(|(f, _)| *f == frame)
                .map(|(_, c)| c);
            let Some(value) = real_at(track, frame) else {
                continue;
            };
            let tick = here.filter(|c| matches!(c.value.as_str(), TICK_1 | TICK_2));
            let starts = row == 0 || here.is_some_and(|c| !is_tick(&c.value));

            // 原画: ringed keys, 中割参考 in a triangle, ○ ●, ×, a line while
            // held and a wave after ×.
            let long = |ticks: bool| held_for(track, frame, ticks, table.duration) >= style.hold;
            if let Some(t) = tick {
                if t.value == style.dot {
                    ring(page, ax, mid, 2.2, true);
                } else {
                    ring(page, ax, mid, 3.2, false);
                }
            } else if starts && value == NULL_CELL {
                page.text(
                    "×",
                    ax as f32,
                    mid as f32,
                    14.0,
                    Align::Center,
                    page.pen,
                    None,
                );
            } else if starts && mark(value).is_some() {
                match here {
                    Some(cell) => keyed(
                        page,
                        ax,
                        mid,
                        value,
                        cell.options.iter().any(|o| o == REFERENCEFRAME),
                    ),
                    // Held from the block before: the number again, not a new key.
                    None => page.text(
                        value,
                        ax as f32,
                        mid as f32,
                        12.0,
                        Align::Center,
                        page.pen,
                        Some(col as f32 - 4.0),
                    ),
                }
            } else if value == NULL_CELL || long(false) {
                hold(page, ax, y, value);
            }
            if style.action_only {
                continue;
            }

            // 動画: the number where a drawing starts (an in-between too), a
            // line while held, × and a wave.
            let number = match numbers {
                Some(map) if value != NULL_CELL => map.get(&frame).cloned().or_else(|| {
                    if row == 0 {
                        last_number(map, frame)
                    } else {
                        None
                    }
                }),
                _ if starts => mark(value),
                _ => None,
            };
            match number {
                Some(text) => page.text(
                    &text,
                    cx as f32,
                    mid as f32,
                    14.0,
                    Align::Center,
                    page.pen,
                    Some(col as f32 - 4.0),
                ),
                // The wave after × at any length; a hold line only for a long hold.
                None if value == NULL_CELL || long(numbers.is_some()) => hold(page, cx, y, value),
                None => {}
            }
        }
    }
    // The end of the cut, across the whole block.
    if (first..first + HALF).contains(&table.duration) {
        let y = grid + (table.duration - first) as i32 * ROW;
        page.hline(action_x, right, y - 1, 3, page.pen);
    }
}

/// A held cel: a line down the middle; after ×, a wavy one.
fn hold(page: &mut Page, cx: i32, y: i32, value: &str) {
    if value == NULL_CELL {
        page.wave(cx + 1, y, y + ROW + 1, page.pen);
    } else {
        page.vline(cx, y, y + ROW + 1, 2, page.pen);
    }
}

/// The 動画 number in effect at `frame` (for the top of a block).
fn last_number(map: &HashMap<usize, String>, frame: usize) -> Option<String> {
    map.iter()
        .filter(|(f, _)| **f <= frame)
        .max_by_key(|(f, _)| **f)
        .map(|(_, n)| n.clone())
}

/// Draw the sheet and write its pages as PNG: `<base>.png` for one page,
/// `<base>_1.png`, `<base>_2.png` … for more. Returns the files written.
pub fn write_pages(sheet: &Sheet, header: &Header, base: &Path) -> Result<Vec<PathBuf>> {
    let pages = render(sheet, header)?;
    let stem = base
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "sheet".into());
    let mut out = Vec::new();
    for (i, page) in pages.iter().enumerate() {
        let name = if pages.len() == 1 {
            format!("{stem}.png")
        } else {
            format!("{stem}_{}.png", i + 1)
        };
        let path = base.with_file_name(name);
        page.save_with_format(&path, image::ImageFormat::Png)
            .map_err(|e| format!("書き込めません: {e}"))?;
        out.push(path);
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    const SHEET: &str = r#"exchangeDigitalTimeSheet Save Data
{"version":5,"header":{"cut":"1","scene":"1"},"timeTables":[{"name":"t","duration":160,"timeTableHeaders":[{"fieldId":0,"names":["A","BGhi"]}],
"fields":[{"fieldId":0,"tracks":[
{"trackNo":0,"frames":[{"frame":0,"data":[{"id":0,"values":["1"]}]},{"frame":6,"data":[{"id":0,"values":["2"]}]},{"frame":30,"data":[{"id":0,"values":["SYMBOL_NULL_CELL"]}]}]},
{"trackNo":1,"frames":[{"frame":0,"data":[{"id":0,"values":["1"]}]}]}]}]}]}
"#;

    #[test]
    fn a_sheet_becomes_paper_pages() {
        assert_eq!((pages(1, 2), pages(144, 2), pages(145, 2)), (1, 1, 2));
        // More cel columns than A–E: 3-second sheets of A–J.
        assert_eq!(
            (layout(144 * 10, 5), layout(145, 6)),
            ((10, 144), (3, 72)),
            "by cel columns, not length"
        );
        let header = Header {
            title: "YMM".into(),
            episode: "3".into(),
            cut: "c013".into(),
            fps: 24,
            ..Header::default()
        };
        let sheet = xdts::from_str(SHEET).unwrap();
        let images = match render(&sheet, &header) {
            Ok(images) => images,
            Err(e) if e.to_string().contains("フォント") => return, // no Windows fonts here
            Err(e) => panic!("{e}"),
        };
        assert_eq!(images.len(), 2, "160 frames: two pages");
        assert_eq!(images[0].dimensions(), (1240, H));
        // Something was written on the paper (lines, numbers).
        let inked = images[0].pixels().filter(|p| p[1] < 120).count();
        assert!(inked > 10_000, "{inked}");
    }
}
