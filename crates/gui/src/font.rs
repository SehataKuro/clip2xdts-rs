//! 日本語を表示するため、OS に入っているフォントを拾って egui に足す。
//! 見つからなければ既定のフォントのまま（英数字は出る）。

use egui::epaint::text::{FontInsert, FontPriority, InsertFontFamily};
use egui::{FontData, FontFamily};

/// 探す順。先に見つかったものを使う。数字は .ttc の中の何番目の書体か
#[cfg(windows)]
const CANDIDATES: &[(&str, u32)] = &[
    ("C:/Windows/Fonts/YuGothM.ttc", 0),
    ("C:/Windows/Fonts/meiryo.ttc", 0),
    ("C:/Windows/Fonts/msgothic.ttc", 0),
];

#[cfg(target_os = "macos")]
const CANDIDATES: &[(&str, u32)] = &[
    ("/System/Library/Fonts/Hiragino Sans GB.ttc", 0),
    ("/Library/Fonts/Arial Unicode.ttf", 0),
];

#[cfg(not(any(windows, target_os = "macos")))]
const CANDIDATES: &[(&str, u32)] = &[
    ("/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc", 0),
    ("/usr/share/fonts/truetype/noto/NotoSansCJK-Regular.ttc", 0),
];

pub fn install_japanese(ctx: &egui::Context) {
    let Some((bytes, index)) =
        CANDIDATES.iter().find_map(|(p, i)| std::fs::read(p).ok().map(|b| (b, *i)))
    else {
        return;
    };
    let mut data = FontData::from_owned(bytes);
    data.index = index;
    let families = [FontFamily::Proportional, FontFamily::Monospace]
        .into_iter()
        .map(|family| InsertFontFamily { family, priority: FontPriority::Highest })
        .collect();
    ctx.add_font(FontInsert { name: "jp".to_owned(), data, families });
}
