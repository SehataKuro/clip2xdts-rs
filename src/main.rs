//! clip2xdts — CLIP STUDIO PAINT (.clip) からタイムシート (.xdts) を一括書き出し
//!
//!   clip2xdts *.clip               各 .clip の隣に .xdts を作成
//!   clip2xdts -o out/ カット/       フォルダ内の .clip をまとめて変換
//!   clip2xdts --all-timelines ...  全タイムラインを書き出す

mod clip;
mod cmt;
mod inflate;
mod sqlite;
mod xdts;

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;

const USAGE: &str = "\
使い方: clip2xdts [オプション] <.clip ファイル または フォルダ>...

オプション:
  -o, --out <フォルダ>   出力先（省略時は .clip と同じ場所）
      --all-timelines   全タイムラインを書き出す（省略時は選択中のタイムラインのみ）
  -j, --jobs <数>        同時に処理するファイル数（省略時は CPU コア数）
  -h, --help            このヘルプを表示
  -V, --version         バージョンを表示";

fn collect(p: &Path, out: &mut Vec<PathBuf>) {
    if p.is_dir() {
        let Ok(rd) = std::fs::read_dir(p) else { return };
        let mut ents: Vec<PathBuf> = rd.filter_map(|e| e.ok().map(|e| e.path())).collect();
        ents.sort();
        for e in ents {
            if e.is_dir() {
                collect(&e, out);
            } else if e.extension().map(|x| x.eq_ignore_ascii_case("clip")).unwrap_or(false) {
                out.push(e);
            }
        }
    } else {
        out.push(p.to_path_buf());
    }
}

fn run_one(src: &Path, out_dir: Option<&Path>, opt: &clip::Options) -> Result<PathBuf, String> {
    let data = std::fs::read(src).map_err(|e| format!("読み込めません: {}", e))?;
    let x = clip::convert(&data, opt)?;
    let stem = src.file_stem().map(|s| s.to_os_string()).unwrap_or_else(|| "out".into());
    let mut name = stem;
    name.push(".xdts");
    let dir = match out_dir {
        Some(d) => d.to_path_buf(),
        None => src.parent().map(|p| p.to_path_buf()).unwrap_or_else(|| PathBuf::from(".")),
    };
    let dst = dir.join(name);
    std::fs::write(&dst, x.to_string()).map_err(|e| format!("書き込めません: {}", e))?;
    Ok(dst)
}

fn main() {
    let mut args = std::env::args().skip(1);
    let mut inputs = Vec::new();
    let mut out_dir: Option<PathBuf> = None;
    let mut opt = clip::Options { all_timelines: false };
    let mut jobs = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(4);
    while let Some(a) = args.next() {
        match a.as_str() {
            "-h" | "--help" => {
                println!("{}", USAGE);
                return;
            }
            "-V" | "--version" => {
                println!("clip2xdts {}", env!("CARGO_PKG_VERSION"));
                return;
            }
            "-o" | "--out" => out_dir = args.next().map(PathBuf::from),
            "-j" | "--jobs" => {
                jobs = args.next().and_then(|v| v.parse().ok()).unwrap_or(jobs).max(1)
            }
            "--all-timelines" => opt.all_timelines = true,
            s if s.starts_with('-') && s.len() > 1 => {
                eprintln!("不明なオプション: {}\n\n{}", s, USAGE);
                std::process::exit(2);
            }
            _ => inputs.push(PathBuf::from(a)),
        }
    }
    if inputs.is_empty() {
        eprintln!("{}", USAGE);
        std::process::exit(2);
    }
    if let Some(d) = &out_dir {
        if let Err(e) = std::fs::create_dir_all(d) {
            eprintln!("出力フォルダを作れません: {}", e);
            std::process::exit(1);
        }
    }
    let mut files = Vec::new();
    for i in &inputs {
        collect(i, &mut files);
    }

    // ファイル単位で並列処理。結果は入力順に表示する
    let next = AtomicUsize::new(0);
    let results: Mutex<Vec<Option<Result<PathBuf, String>>>> =
        Mutex::new((0..files.len()).map(|_| None).collect());
    std::thread::scope(|s| {
        for _ in 0..jobs.min(files.len().max(1)) {
            s.spawn(|| loop {
                let i = next.fetch_add(1, Ordering::SeqCst);
                if i >= files.len() {
                    break;
                }
                let r = run_one(&files[i], out_dir.as_deref(), &opt);
                results.lock().unwrap()[i] = Some(r);
            });
        }
    });

    let (mut ok, mut ng) = (0, 0);
    for (f, r) in files.iter().zip(results.into_inner().unwrap()) {
        match r {
            Some(Ok(dst)) => {
                println!("OK  {} -> {}", f.display(), dst.display());
                ok += 1;
            }
            Some(Err(e)) => {
                eprintln!("NG  {}: {}", f.display(), e);
                ng += 1;
            }
            None => {}
        }
    }
    println!("完了: 成功 {} / 失敗 {}", ok, ng);
    if ng > 0 {
        std::process::exit(1);
    }
}
