//! clip2xdts — CLIP STUDIO PAINT (.clip) からタイムシートを一括書き出し
//!
//!   clip2xdts *.clip               各 .clip の隣に .xdts を作成
//!   clip2xdts -o out/ カット/       フォルダ内の .clip をまとめて変換
//!   clip2xdts -f tdts ...          東映 DTS 形式 (.tdts) で書き出す
//!   clip2xdts --all-timelines ...  全タイムラインを書き出す

use clip2xdts_core::{clip, collect_clips, run_batch, Format, Job};
use std::path::PathBuf;

const USAGE: &str = "\
使い方: clip2xdts [オプション] <.clip ファイル または フォルダ>...

オプション:
  -o, --out <フォルダ>   出力先（省略時は .clip と同じ場所）
  -f, --format <形式>    xdts / tdts / both（省略時は xdts）
      --all-timelines   全タイムラインを書き出す（省略時は選択中のタイムラインのみ）
  -j, --jobs <数>        同時に処理するファイル数（省略時は CPU コア数）
  -h, --help            このヘルプを表示
  -V, --version         バージョンを表示";

fn main() {
    let mut args = std::env::args().skip(1);
    let mut inputs = Vec::new();
    let mut out_dir: Option<PathBuf> = None;
    let mut options = clip::Options { all_timelines: false };
    let mut format = Format::Xdts;
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
            "-f" | "--format" => {
                let v = args.next().unwrap_or_default();
                match Format::parse(&v) {
                    Some(f) => format = f,
                    None => {
                        eprintln!("形式は xdts / tdts / both のどれかです: {}", v);
                        std::process::exit(2);
                    }
                }
            }
            "-j" | "--jobs" => {
                jobs = args.next().and_then(|v| v.parse().ok()).unwrap_or(jobs).max(1)
            }
            "--all-timelines" => options.all_timelines = true,
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
        collect_clips(i, &mut files);
    }
    let job = Job { out_dir: out_dir.as_deref(), format, options, jobs };
    let results = run_batch(&files, &job, || {});

    let (mut ok, mut ng) = (0, 0);
    for (f, r) in files.iter().zip(results) {
        match r {
            Ok(dsts) => {
                for dst in &dsts {
                    println!("OK  {} -> {}", f.display(), dst.display());
                }
                ok += 1;
            }
            Err(e) => {
                eprintln!("NG  {}: {}", f.display(), e);
                ng += 1;
            }
        }
    }
    println!("完了: 成功 {} / 失敗 {}", ok, ng);
    if ng > 0 {
        std::process::exit(1);
    }
}
