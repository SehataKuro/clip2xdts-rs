//! .clip ファイルの読み取りとタイムシート情報の組み立て

use crate::sqlite::{Db, Row};
use crate::xdts::{Frame, TimeTable, Track, Xdts, NULL_CELL};
use crate::{cmt, inflate};
use std::collections::HashMap;

pub struct Options {
    pub all_timelines: bool,
}

/// .clip のチャンクを走査して SQLite 部分の範囲を返す
fn find_sqlite(d: &[u8]) -> Result<&[u8], String> {
    if d.len() < 24 || &d[..8] != b"CSFCHUNK" {
        return Err("CLIP STUDIO のファイルではありません".into());
    }
    let mut p = 24;
    while p + 16 <= d.len() {
        let name = &d[p..p + 8];
        let size = be64(&d[p + 8..]) as usize;
        let body = p + 16;
        if body + size > d.len() {
            break;
        }
        if name == b"CHNKSQLi" {
            return Ok(&d[body..body + size]);
        }
        if name == b"CHNKFoot" {
            break;
        }
        p = body + size;
    }
    Err("SQLite チャンクが見つかりません（ファイルが壊れている可能性があります）".into())
}

fn be64(b: &[u8]) -> u64 {
    let mut a = [0u8; 8];
    a.copy_from_slice(&b[..8]);
    u64::from_be_bytes(a)
}

struct Clip<'a> {
    data: &'a [u8],
    db: Db<'a>,
    ext: HashMap<Vec<u8>, usize>,
}

impl<'a> Clip<'a> {
    /// ExternalChunk の中身（zlib 展開済み）
    fn external(&self, id: &[u8]) -> Result<Option<Vec<u8>>, String> {
        let Some(&off) = self.ext.get(id) else { return Ok(None) };
        let d = self.data;
        if off + 24 > d.len() || &d[off..off + 8] != b"CHNKExta" {
            return Ok(None);
        }
        let mut p = off + 16;
        let idlen = be64(&d[p..]) as usize;
        p += 8 + idlen;
        if p + 8 > d.len() {
            return Ok(None);
        }
        let dsize = be64(&d[p..]) as usize;
        p += 8;
        let body = d.get(p..p + dsize).ok_or("外部チャンクが途中で終わっています")?;
        if body.len() > 4 && body[4] == 0x78 {
            Ok(Some(inflate::zlib_decompress(&body[4..])?))
        } else {
            Ok(Some(body.to_vec()))
        }
    }
}

fn uuid_key(v: &[u8], is_text: bool) -> String {
    if is_text {
        String::from_utf8_lossy(v).replace('-', "").to_ascii_lowercase()
    } else {
        v.iter().map(|b| format!("{:02x}", b)).collect()
    }
}

/// Python の round() と同じ偶数丸め
fn round_frame(x: f64) -> i64 {
    x.round_ties_even() as i64
}

/// タイムライン 1 本につき 1 枚のシートを返す。
/// 選択中だけなら 1 枚、`all_timelines` ならファイル内のタイムラインの数だけ返る
pub fn convert(file: &[u8], opt: &Options) -> Result<Vec<Xdts>, String> {
    let db = Db::open(find_sqlite(file)?)?;
    let mut ext = HashMap::new();
    if db.has_table("ExternalChunk") {
        for r in db.rows("ExternalChunk")? {
            if let Some(id) = r.get("ExternalID").as_bytes() {
                ext.insert(id.to_vec(), r.int("Offset") as usize);
            }
        }
    }
    let clip = Clip { data: file, db, ext };
    let db = &clip.db;

    if !db.has_table("AnimationCutBank") || !db.has_table("TimeLine") {
        return Err("アニメーションのタイムラインがありません".into());
    }
    let bank = db.rows("AnimationCutBank")?;
    let bank = bank.first().ok_or("タイムラインがありません")?;

    // タイムラインの連結リスト
    let tl_rows = db.rows("TimeLine")?;
    let tl_by_id: HashMap<i64, &Row> = tl_rows.iter().map(|r| (r.int("MainId"), r)).collect();
    let mut tls = Vec::new();
    let mut t = bank.int("FirstTimeLine");
    while t != 0 {
        let r = *tl_by_id.get(&t).ok_or("タイムラインの参照が壊れています")?;
        if tls.len() > tl_rows.len() {
            return Err("タイムラインの参照が循環しています".into());
        }
        tls.push(r);
        t = r.int("NextTimeLine");
    }
    if tls.is_empty() {
        return Err("タイムラインがありません".into());
    }
    let cur = (bank.int("CurrentIndex").max(0) as usize).min(tls.len() - 1);
    let targets: Vec<&Row> = if opt.all_timelines { tls.clone() } else { vec![tls[cur]] };

    let folders = visible_anim_folders(db)?;
    let track_rows = db.rows("Track")?;
    let tr_by_id: HashMap<i64, &Row> = track_rows.iter().map(|r| (r.int("MainId"), r)).collect();

    let mut sheets = Vec::new();
    for tl in &targets {
        let fps = tl.get("FrameRate").as_f64().unwrap_or(24.0);
        let start = tl.get("StartFrame").as_f64().unwrap_or(0.0);
        let end = tl.get("EndFrame").as_f64().unwrap_or(0.0);

        // このタイムラインのトラックを レイヤーUUID で引けるようにする
        let mut tracks: HashMap<String, &Row> = HashMap::new();
        let mut tr = tl.int("FirstTrack");
        let mut guard = 0;
        while tr != 0 {
            let Some(row) = tr_by_id.get(&tr) else { break };
            if let Some(u) = row.get("LayerUuidWithTrack").as_bytes() {
                let is_text = row.get("LayerUuidWithTrack").as_str().is_some();
                tracks.insert(uuid_key(u, is_text), row);
            }
            tr = row.int("TrackNextIndex");
            guard += 1;
            if guard > track_rows.len() {
                break;
            }
        }

        let mut names = Vec::new();
        let mut xtracks = Vec::new();
        for (name, uuid) in &folders {
            let Some(row) = tracks.get(uuid) else { continue };
            names.push(name.clone());
            let (keys, end_frame) = track_keys(&clip, row, fps)?;
            let mut frames: Vec<Frame> = Vec::new();
            for (fr, nm) in keys {
                let v = nm.unwrap_or_else(|| NULL_CELL.to_string());
                if frames.last().map(|f| f.value == v).unwrap_or(false) {
                    continue;
                }
                frames.push(Frame { frame: fr, value: v });
            }
            match (frames.last(), end_frame) {
                (None, _) => frames.push(Frame { frame: 0, value: NULL_CELL.into() }),
                (Some(l), Some(e)) if l.value != NULL_CELL => {
                    frames.push(Frame { frame: e, value: NULL_CELL.into() })
                }
                _ => {}
            }
            xtracks.push(Track { frames });
        }
        sheets.push(Xdts {
            cut: tl.int("CutIndexForName").to_string(),
            scene: tl.int("SceneIndexForName").to_string(),
            tables: vec![TimeTable {
                name: tl.get("TimeLineName").as_str().unwrap_or("").to_string(),
                duration: round_frame(end - start),
                names,
                tracks: xtracks,
            }],
        });
    }
    Ok(sheets)
}

/// 表示中（親フォルダも表示）のアニメーションフォルダを (名前, UUIDキー) でパレット順に返す
fn visible_anim_folders(db: &Db) -> Result<Vec<(String, String)>, String> {
    let rows = db.rows("Layer")?;
    let by_id: HashMap<i64, &Row> = rows.iter().map(|r| (r.int("MainId"), r)).collect();
    let canvas = db.rows("Canvas")?;
    let root = canvas.first().ok_or("キャンバスがありません")?.int("CanvasRootFolder");
    let root = *by_id.get(&root).ok_or("ルートフォルダがありません")?;
    let mut out = Vec::new();
    let mut visited = 0usize;
    fn walk(
        id: i64,
        vis: bool,
        by_id: &HashMap<i64, &Row>,
        out: &mut Vec<(String, String)>,
        visited: &mut usize,
        depth: usize,
    ) {
        let mut i = id;
        while i != 0 {
            *visited += 1;
            if *visited > by_id.len() * 2 || depth > 128 {
                return;
            }
            let Some(r) = by_id.get(&i) else { return };
            let v = vis && (r.int("LayerVisibility") & 1) != 0;
            if r.int("AnimationFolder") != 0 {
                if v {
                    let uuid = r.get("LayerUuid");
                    let key = uuid
                        .as_bytes()
                        .map(|b| uuid_key(b, uuid.as_str().is_some()))
                        .unwrap_or_default();
                    out.push((r.get("LayerName").as_str().unwrap_or("").to_string(), key));
                }
            } else if r.int("LayerFirstChildIndex") != 0 {
                walk(r.int("LayerFirstChildIndex"), v, by_id, out, visited, depth + 1);
            }
            i = r.int("LayerNextIndex");
        }
    }
    walk(root.int("LayerFirstChildIndex"), true, &by_id, &mut out, &mut visited, 0);
    Ok(out)
}

/// トラックの (コマ, セル名 / None=空セル) と、クリップ終端のコマを返す
fn track_keys(clip: &Clip, row: &Row, fps: f64) -> Result<(Vec<(i64, Option<String>)>, Option<i64>), String> {
    let mut raw = None;
    for col in ["TrackActionMixer2", "TrackActionMixer"] {
        if let Some(id) = row.get(col).as_bytes() {
            if let Some(d) = clip.external(id)? {
                raw = Some(d);
                break;
            }
        }
    }
    let Some(raw) = raw else { return Ok((Vec::new(), None)) };
    let doc = cmt::parse(&raw)?;
    let mut keys = Vec::new();
    let mut end_frame = None;
    let mut clips = Vec::new();
    doc.find_all("ActionNodeClip", &mut clips);
    for c in clips {
        let mut rate = 60.0;
        if let Some(tc) = c.child("TimeClip") {
            if let Some(r) = tc.child("Rate").and_then(|n| n.f64()) {
                rate = r;
            }
            if let Some(e) = tc.child("End").and_then(|n| n.f64()) {
                end_frame = Some(round_frame(e * fps / rate));
            }
        }
        let mut curves = Vec::new();
        c.find_all("FCurve", &mut curves);
        for fc in curves {
            if fc.attr("Type") != Some("ImageCelName") {
                continue;
            }
            let frames = match fc.child("Frame").map(|n| &n.val) {
                Some(cmt::Val::F64s(v)) => v.clone(),
                _ => Vec::new(),
            };
            let tags = match fc.child("Tag").map(|n| &n.val) {
                Some(cmt::Val::Strs(v)) => v.clone(),
                _ => Vec::new(),
            };
            let values = match fc.child("Value").map(|n| &n.val) {
                Some(cmt::Val::F64s(v)) => v.clone(),
                _ => Vec::new(),
            };
            for (i, t) in frames.iter().enumerate() {
                let mut name = tags.get(i).cloned();
                if name.as_deref() == Some("") || values.get(i).map(|v| *v < 0.0).unwrap_or(false) {
                    name = None;
                }
                keys.push((round_frame(t * fps / rate), name));
            }
        }
    }
    keys.sort_by_key(|k| k.0);
    Ok((keys, end_frame))
}
