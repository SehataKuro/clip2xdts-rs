//! 入力の収集と、ファイル単位の変換・並列実行。CLI と GUI が共用する。

use crate::{clip, tdts};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;

/// 書き出す形式
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Format {
    Xdts,
    Tdts,
    Both,
}

impl Format {
    /// 書き出す拡張子を順に返す
    pub fn exts(self) -> &'static [&'static str] {
        match self {
            Format::Xdts => &["xdts"],
            Format::Tdts => &["tdts"],
            Format::Both => &["xdts", "tdts"],
        }
    }

    /// `-f` に渡せる名前から読む
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "xdts" => Some(Format::Xdts),
            "tdts" => Some(Format::Tdts),
            "both" => Some(Format::Both),
            _ => None,
        }
    }
}

/// 一括変換の指定
pub struct Job<'a> {
    pub out_dir: Option<&'a Path>,
    pub format: Format,
    pub options: clip::Options,
    pub jobs: usize,
}

/// ファイルなら自身、フォルダならサブフォルダまで辿って .clip を集める
pub fn collect_clips(p: &Path, out: &mut Vec<PathBuf>) {
    if p.is_dir() {
        let Ok(rd) = std::fs::read_dir(p) else { return };
        let mut ents: Vec<PathBuf> = rd.filter_map(|e| e.ok().map(|e| e.path())).collect();
        ents.sort();
        for e in ents {
            if e.is_dir() {
                collect_clips(&e, out);
            } else if e.extension().map(|x| x.eq_ignore_ascii_case("clip")).unwrap_or(false) {
                out.push(e);
            }
        }
    } else {
        out.push(p.to_path_buf());
    }
}

/// 1 ファイルを変換し、書き出した先を返す
pub fn convert_one(src: &Path, job: &Job) -> Result<Vec<PathBuf>, String> {
    let data = std::fs::read(src).map_err(|e| format!("読み込めません: {}", e))?;
    let sheets = clip::convert(&data, &job.options)?;
    let stem = src.file_stem().map(|s| s.to_os_string()).unwrap_or_else(|| "out".into());
    let dir = match job.out_dir {
        Some(d) => d.to_path_buf(),
        None => src.parent().map(|p| p.to_path_buf()).unwrap_or_else(|| PathBuf::from(".")),
    };
    let mut dsts = Vec::new();
    for (i, sheet) in sheets.iter().enumerate() {
        for ext in job.format.exts() {
            let dst = dir.join(sheet_name(&stem, i, sheets.len(), ext));
            let body = match *ext {
                "tdts" => tdts::to_string(sheet),
                _ => sheet.to_string(),
            };
            std::fs::write(&dst, body).map_err(|e| format!("書き込めません: {}", e))?;
            dsts.push(dst);
        }
    }
    Ok(dsts)
}

/// 書き出すファイル名。シートが 1 枚なら `a.xdts`、
/// 複数なら `a_sheet1.xdts` `a_sheet2.xdts` … と連番を付ける
fn sheet_name(stem: &std::ffi::OsStr, i: usize, total: usize, ext: &str) -> std::ffi::OsString {
    let mut name = stem.to_os_string();
    if total > 1 {
        name.push(format!("_sheet{}", i + 1));
    }
    name.push(".");
    name.push(ext);
    name
}

/// ファイル単位で並列に変換する。結果は入力と同じ順で返る。
/// `done` は 1 ファイル終わるたびに呼ばれる（進捗表示用。呼ばれる順は不定）
pub fn run_batch(
    files: &[PathBuf],
    job: &Job,
    done: impl Fn() + Sync,
) -> Vec<Result<Vec<PathBuf>, String>> {
    let next = AtomicUsize::new(0);
    let results: Mutex<Vec<Option<Result<Vec<PathBuf>, String>>>> =
        Mutex::new((0..files.len()).map(|_| None).collect());
    std::thread::scope(|s| {
        for _ in 0..job.jobs.max(1).min(files.len().max(1)) {
            s.spawn(|| loop {
                let i = next.fetch_add(1, Ordering::SeqCst);
                if i >= files.len() {
                    break;
                }
                let r = convert_one(&files[i], job);
                results.lock().unwrap()[i] = Some(r);
                done();
            });
        }
    });
    results
        .into_inner()
        .unwrap()
        .into_iter()
        .map(|r| r.unwrap_or_else(|| Err("処理されませんでした".into())))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::sheet_name;
    use std::ffi::OsStr;

    #[test]
    fn single_sheet_keeps_the_plain_name() {
        assert_eq!(sheet_name(OsStr::new("a"), 0, 1, "xdts"), OsStr::new("a.xdts"));
        assert_eq!(sheet_name(OsStr::new("カット001"), 0, 1, "tdts"), OsStr::new("カット001.tdts"));
    }

    #[test]
    fn several_sheets_get_a_number() {
        assert_eq!(sheet_name(OsStr::new("a"), 0, 3, "xdts"), OsStr::new("a_sheet1.xdts"));
        assert_eq!(sheet_name(OsStr::new("a"), 1, 3, "xdts"), OsStr::new("a_sheet2.xdts"));
        assert_eq!(sheet_name(OsStr::new("a"), 2, 3, "tdts"), OsStr::new("a_sheet3.tdts"));
    }

    #[test]
    fn a_dot_in_the_name_is_left_alone() {
        assert_eq!(sheet_name(OsStr::new("c.01"), 1, 2, "xdts"), OsStr::new("c.01_sheet2.xdts"));
    }
}
