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
    let x = clip::convert(&data, &job.options)?;
    let stem = src.file_stem().map(|s| s.to_os_string()).unwrap_or_else(|| "out".into());
    let dir = match job.out_dir {
        Some(d) => d.to_path_buf(),
        None => src.parent().map(|p| p.to_path_buf()).unwrap_or_else(|| PathBuf::from(".")),
    };
    let mut dsts = Vec::new();
    for ext in job.format.exts() {
        let mut name = stem.clone();
        name.push(".");
        name.push(ext);
        let dst = dir.join(name);
        let body = match *ext {
            "tdts" => tdts::to_string(&x),
            _ => x.to_string(),
        };
        std::fs::write(&dst, body).map_err(|e| format!("書き込めません: {}", e))?;
        dsts.push(dst);
    }
    Ok(dsts)
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
