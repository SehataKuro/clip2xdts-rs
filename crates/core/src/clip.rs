//! .clip ファイルの読み取りとタイムシート情報の組み立て

use crate::sheet::{NULL_CELL, Sheet, TimeTable, Track};
use crate::sqlite::{Db, Row};
use crate::{cmt, inflate};
use std::collections::HashMap;

/// 東映 DTS が保存したシートが持つダミーコマ数。頭に 1 秒、尻に 2 秒ぶん。
/// 実機の保存ファイル 2 本（別の作品のものを含む）がどちらもこの値だった
pub(crate) const HEAD_DUMMY: usize = 24;
pub(crate) const FOOT_DUMMY: usize = 48;

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
        let Some(&off) = self.ext.get(id) else {
            return Ok(None);
        };
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
        let body = d
            .get(p..p + dsize)
            .ok_or("外部チャンクが途中で終わっています")?;
        if body.len() > 4 && body[4] == 0x78 {
            Ok(Some(inflate::zlib_decompress(&body[4..])?))
        } else {
            Ok(Some(body.to_vec()))
        }
    }
}

fn uuid_key(v: &[u8], is_text: bool) -> String {
    if is_text {
        String::from_utf8_lossy(v)
            .replace('-', "")
            .to_ascii_lowercase()
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
pub fn convert(file: &[u8], opt: &Options) -> Result<Vec<Sheet>, String> {
    let db = Db::open(find_sqlite(file)?)?;
    let mut ext = HashMap::new();
    if db.has_table("ExternalChunk") {
        for r in db.rows("ExternalChunk")? {
            if let Some(id) = r.get("ExternalID").as_bytes() {
                ext.insert(id.to_vec(), r.int("Offset") as usize);
            }
        }
    }
    let clip = Clip {
        data: file,
        db,
        ext,
    };
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
    let targets: Vec<&Row> = if opt.all_timelines {
        tls.clone()
    } else {
        vec![tls[cur]]
    };

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

        let duration = round_frame(end - start).max(0) as usize;
        let mut names = Vec::new();
        let mut xtracks = Vec::new();
        for (name, uuid) in &folders {
            let Some(row) = tracks.get(uuid) else {
                continue;
            };
            let (keys, end_frame) = track_keys(&clip, row, fps, start, end)?;
            let Some(frames) = export_frames(keys, end_frame, duration) else {
                continue;
            };
            names.push(name.clone());
            // 変化点は文字列で畳んでから、クレートの Cell に移す（p1 のマークは .clip から出ない）
            xtracks.push(Track {
                frames: frames.into_iter().map(|(f, v)| (f, v.into())).collect(),
            });
        }
        sheets.push(Sheet {
            cut: tl.int("CutIndexForName").to_string(),
            scene: tl.int("SceneIndexForName").to_string(),
            tables: vec![TimeTable {
                name: tl.get("TimeLineName").as_str().unwrap_or("").to_string(),
                duration,
                columns: names,
                tracks: xtracks,
                // 東映 DTS が保存したシートは頭に 24 コマ・尻に 48 コマの
                // ダミーを持つ。.clip には無い情報なので、実機の値で埋める
                // （.xdts には出ない。TDTS だけが持つ項目）
                head_dummy: HEAD_DUMMY,
                foot_dummy: FOOT_DUMMY,
                // 台詞欄・カメラ欄は .clip に情報が無いので空のまま
                ..TimeTable::default()
            }],
            ..Sheet::default()
        });
    }
    Ok(sheets)
}

/// トラックの変化点を、シートに載る範囲（0 〜 尺-1）に切り詰めて畳む。
///
/// 尺より前のコマ（マイナス）で最後に出ていたセルは 0 コマ目へ持ち越す（空セルは持ち越さない）。
/// 尺以降の変化点は捨て、クリップ終端の空セルだけは尺を越えていても残す。
/// 載せる変化点が 1 つも無いトラックは None（列ごと出さない）
fn export_frames(
    keys: Vec<(i64, Option<String>)>,
    end_frame: Option<i64>,
    duration: usize,
) -> Option<Vec<(usize, String)>> {
    let last = duration.max(1) as i64 - 1;
    let mut carry = None;
    let mut inside = Vec::new();
    for (f, v) in keys.into_iter().filter(|k| k.0 <= last) {
        if f < 0 {
            carry = Some(v);
        } else {
            inside.push((f, v));
        }
    }
    if let Some(Some(v)) = carry
        && inside.first().is_none_or(|k| k.0 != 0)
    {
        inside.insert(0, (0, Some(v)));
    }
    if inside.is_empty() {
        return None;
    }
    if let Some(e) = end_frame
        && e >= 0
        && e > inside.last().map_or(i64::MIN, |k| k.0)
    {
        inside.push((e, None));
    }
    // 共有クレートの変化点モデル。フレーム番号は 0 起点の usize。同じセルが続く変化点は畳む
    let mut frames: Vec<(usize, String)> = Vec::new();
    for (f, v) in inside {
        let v = v.unwrap_or_else(|| NULL_CELL.to_string());
        if frames.last().is_some_and(|(_, prev)| *prev == v) {
            continue;
        }
        frames.push((f as usize, v));
    }
    Some(frames)
}

/// 表示中（親フォルダも表示）のアニメーションフォルダを (名前, UUIDキー) でパレット順に返す
fn visible_anim_folders(db: &Db) -> Result<Vec<(String, String)>, String> {
    let rows = db.rows("Layer")?;
    let by_id: HashMap<i64, &Row> = rows.iter().map(|r| (r.int("MainId"), r)).collect();
    let canvas = db.rows("Canvas")?;
    let root = canvas
        .first()
        .ok_or("キャンバスがありません")?
        .int("CanvasRootFolder");
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
                walk(
                    r.int("LayerFirstChildIndex"),
                    v,
                    by_id,
                    out,
                    visited,
                    depth + 1,
                );
            }
            i = r.int("LayerNextIndex");
        }
    }
    walk(
        root.int("LayerFirstChildIndex"),
        true,
        &by_id,
        &mut out,
        &mut visited,
        0,
    );
    Ok(out)
}

/// トラックの変化点 (コマ, セル名 / None=空セル) と、クリップ終端のコマ
type TrackKeys = (Vec<(i64, Option<String>)>, Option<i64>);

/// ActionNodeClip 1 本ぶんのセル露出。フレームはタイムラインの絶対フレーム（丸め前）
struct CelClip {
    start: f64,
    end: f64,
    keys: Vec<(f64, Option<String>)>,
}

/// 整数に十分近ければ整数に寄せる（浮動小数の誤差を消す）
fn frame_coordinate(v: f64) -> f64 {
    let r = v.round();
    if (v - r).abs() < 1e-7 { r } else { v }
}

/// JavaScript の Math.round と同じ（.5 は +∞ 側）
fn js_round(x: f64) -> f64 {
    (x + 0.5).floor()
}

fn num(node: &cmt::Node, name: &str) -> Result<f64, String> {
    node.child(name)
        .and_then(|n| n.f64())
        .filter(|v| v.is_finite())
        .ok_or_else(|| format!("mixer: {} の時間の値が読めません", node.name))
}

fn f64s(node: Option<&cmt::Node>) -> Option<Vec<f64>> {
    match node.map(|n| &n.val) {
        Some(cmt::Val::F64s(v)) => Some(v.clone()),
        Some(cmt::Val::I32s(v)) => Some(v.iter().map(|x| *x as f64).collect()),
        _ => None,
    }
}

/// ActionNodeClip 1 本を読む。セル（ImageCelName）を持たないクリップは None。
///
/// キーの時刻はクリップ内のローカル時間（TimeInfo.Rate 単位）。それを
/// MotionClip（素材側の使用範囲）から TimeClip（タイムライン上の配置）へ写して
/// タイムラインのフレームにする
fn cel_clip(c: &cmt::Node, fps: f64) -> Result<Option<CelClip>, String> {
    let Some(anim) = c.child("AnimInfo") else {
        return Ok(None);
    };
    let curve = anim
        .kids
        .iter()
        .find(|n| n.name == "FCurve" && n.attr("Type") == Some("ImageCelName"));
    let param = anim
        .kids
        .iter()
        .find(|n| n.name == "Param" && n.attr("Name") == Some("ImageCelName"));
    if curve.is_none() && param.is_none() {
        return Ok(None);
    }
    let (Some(time), Some(motion), Some(info)) = (
        c.child("TimeClip"),
        c.child("MotionClip"),
        c.child("TimeInfo"),
    ) else {
        return Err("mixer: クリップの時間情報がありません".into());
    };
    let (time_rate, motion_rate, curve_rate) =
        (num(time, "Rate")?, num(motion, "Rate")?, num(info, "Rate")?);
    if !(time_rate > 0.0 && motion_rate > 0.0 && curve_rate > 0.0) {
        return Err("mixer: クリップの時間の単位が不正です".into());
    }
    let time_start = num(time, "Start")? / time_rate;
    let time_end = num(time, "End")? / time_rate;
    let motion_start = num(motion, "Start")? / motion_rate;
    let motion_end = num(motion, "End")? / motion_rate;
    if !(time_end > time_start && motion_end > motion_start) {
        return Err("mixer: クリップの時間範囲が不正です".into());
    }
    let start = frame_coordinate(time_start * fps);
    let end = frame_coordinate(time_end * fps);

    let default_tag = param.and_then(|p| p.attr("Tag")).filter(|t| !t.is_empty());
    let mut channel: Vec<(f64, Option<String>)> = Vec::new();
    if let Some(fc) = curve {
        let frames = f64s(fc.child("Frame")).unwrap_or_default();
        let values = f64s(fc.child("Value"));
        let tags = match fc.child("Tag").map(|n| &n.val) {
            Some(cmt::Val::Strs(v)) => Some(v.clone()),
            _ => None,
        };
        // Tag の無い曲線は、Param があって既定タグが空で Value が全部 0 のときだけ空セル扱い
        let tags = match tags {
            Some(t) => t,
            None if param.is_some()
                && default_tag.is_none()
                && values
                    .as_ref()
                    .is_some_and(|v| v.len() == frames.len() && v.iter().all(|x| *x == 0.0)) =>
            {
                vec![String::new(); frames.len()]
            }
            None => return Err("mixer: セルのキーが不正です".into()),
        };
        if tags.len() != frames.len() {
            return Err("mixer: セルのキーが不正です".into());
        }
        let mut prev = f64::NEG_INFINITY;
        for (f, t) in frames.iter().zip(tags) {
            if !(f.is_finite() && *f > prev) {
                return Err("mixer: セルのキーが不正です".into());
            }
            prev = *f;
            channel.push((*f, (!t.is_empty()).then_some(t)));
        }
    }

    let mut keys = Vec::new();
    let mut carry = default_tag.map(str::to_string);
    for (tick, value) in channel {
        let local = tick / curve_rate;
        if local <= motion_start {
            carry = value.clone();
        }
        if local < motion_start || local >= motion_end {
            continue;
        }
        let f = frame_coordinate(
            (time_start
                + (local - motion_start) * (time_end - time_start) / (motion_end - motion_start))
                * fps,
        );
        keys.push((f, value));
    }
    // クリップの頭にキーが無ければ、直前の（または既定の）セルがクリップ頭から出る
    if keys.first().is_none_or(|k| k.0 > start) {
        keys.insert(0, (start, carry));
    }
    Ok(Some(CelClip { start, end, keys }))
}

/// トラックの変化点 (コマ, セル名 / None=空セル) と、クリップ終端のコマを返す。
/// コマはタイムラインの StartFrame を 0 とした値
fn track_keys(
    clip: &Clip,
    row: &Row,
    fps: f64,
    tl_start: f64,
    tl_end: f64,
) -> Result<TrackKeys, String> {
    let mut raw = None;
    for col in ["TrackActionMixer2", "TrackActionMixer"] {
        if let Some(id) = row.get(col).as_bytes() {
            if let Some(d) = clip.external(id)? {
                raw = Some(d);
                break;
            }
        }
    }
    // セルのデータが無いトラックは空セルだけの列
    let Some(raw) = raw else {
        return Ok((vec![(0, None)], None));
    };
    mixer_keys(&cmt::parse(&raw)?, fps, tl_start, tl_end)
}

/// TrackActionMixer の木から、トラックの変化点とクリップ終端のコマを組み立てる
fn mixer_keys(doc: &cmt::Node, fps: f64, tl_start: f64, tl_end: f64) -> Result<TrackKeys, String> {
    let mut nodes = Vec::new();
    doc.find_all("ActionNodeClip", &mut nodes);
    let mut clips = Vec::new();
    for c in nodes {
        if let Some(cc) = cel_clip(c, fps)? {
            clips.push(cc);
        }
    }
    if clips.is_empty() {
        return Ok((vec![(0, None)], None));
    }
    clips.sort_by(|a, b| a.start.total_cmp(&b.start));
    if clips.windows(2).any(|w| w[0].end > w[1].start) {
        return Err("セルのクリップが重なっています".into());
    }

    let work_start = tl_start.min(clips[0].start);
    let work_end = clips
        .iter()
        .filter_map(|c| c.keys.last().map(|k| k.0 + 1.0))
        .fold(tl_end, f64::max);
    // 同じコマに複数の変化点が来たら後のものが勝つ
    let mut events: Vec<(f64, Option<String>)> = vec![(work_start, None)];
    let mut set = |f: f64, v: Option<String>| match events.iter_mut().find(|e| e.0 == f) {
        Some(e) => e.1 = v,
        None => events.push((f, v)),
    };
    for c in &clips {
        for (f, v) in &c.keys {
            set(*f, v.clone());
        }
        if c.end < work_end {
            set(c.end, None);
        }
    }
    events.sort_by(|a, b| a.0.total_cmp(&b.0));

    let to_int = |f: f64, what: &str| -> Result<i64, String> {
        let r = js_round(f);
        if (f - r).abs() >= 1e-3 {
            return Err(format!("{}が整数フレームに一致しません", what));
        }
        Ok(r as i64)
    };
    let mut keys = Vec::with_capacity(events.len());
    for (f, v) in events {
        keys.push((to_int(f - tl_start, "セル露出の位置")?, v));
    }
    let raw_end = clips
        .iter()
        .map(|c| c.end)
        .fold(f64::NEG_INFINITY, f64::max)
        - tl_start;
    Ok((keys, Some(to_int(raw_end, "セル露出の終端")?)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use cmt::{Node, Val};

    fn node(name: &str, val: Val, attrs: &[(&str, &str)], kids: Vec<Node>) -> Node {
        Node {
            name: name.into(),
            val,
            attrs: attrs
                .iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect(),
            kids,
        }
    }

    fn range(name: &str, start: f64, end: f64, rate: f64) -> Node {
        node(
            name,
            Val::None,
            &[],
            vec![
                node("Start", Val::F64(start), &[], vec![]),
                node("End", Val::F64(end), &[], vec![]),
                node("Rate", Val::F64(rate), &[], vec![]),
            ],
        )
    }

    /// セルのクリップ 1 本。時間はすべてフレーム（24fps）で渡し、CSP と同じく
    /// TimeClip/MotionClip は 60 単位、キーは TimeInfo.Rate=24 で持たせる
    fn cel_clip_node(time: (f64, f64), motion: (f64, f64), keys: &[(f64, &str)]) -> Node {
        let s = 60.0 / 24.0;
        node(
            "ActionNodeClip",
            Val::None,
            &[],
            vec![
                range("TimeClip", time.0 * s, time.1 * s, 60.0),
                range("MotionClip", motion.0 * s, motion.1 * s, 60.0),
                node(
                    "TimeInfo",
                    Val::None,
                    &[],
                    vec![node("Rate", Val::F64(24.0), &[], vec![])],
                ),
                node(
                    "AnimInfo",
                    Val::None,
                    &[],
                    vec![node(
                        "FCurve",
                        Val::None,
                        &[("Type", "ImageCelName")],
                        vec![
                            node(
                                "Frame",
                                Val::F64s(keys.iter().map(|k| k.0).collect()),
                                &[],
                                vec![],
                            ),
                            node(
                                "Tag",
                                Val::Strs(keys.iter().map(|k| k.1.to_string()).collect()),
                                &[],
                                vec![],
                            ),
                        ],
                    )],
                ),
            ],
        )
    }

    fn general(clips: Vec<Node>) -> Node {
        node("General", Val::None, &[], clips)
    }

    fn sheet_frames(doc: &Node, duration: usize) -> Vec<(usize, String)> {
        let (keys, end) = mixer_keys(doc, 24.0, 0.0, duration as f64).unwrap();
        export_frames(keys, end, duration).unwrap()
    }

    fn fr(v: &[(usize, &str)]) -> Vec<(usize, String)> {
        v.iter().map(|(f, s)| (*f, s.to_string())).collect()
    }

    /// タイムライン上でクリップを後ろへ置いたら、キーもその位置から出る。
    /// 頭の空きは空セル（C12 (1).clip: 35 コマ目に置かれたクリップが 0 コマ目から出ていた）
    #[test]
    fn clip_placed_later_on_timeline() {
        let doc = general(vec![cel_clip_node(
            (35.0, 80.0),
            (0.0, 45.0),
            &[(0.0, "1"), (1.0, "1a"), (2.0, "2")],
        )]);
        assert_eq!(
            sheet_frames(&doc, 80),
            fr(&[
                (0, NULL_CELL),
                (35, "1"),
                (36, "1a"),
                (37, "2"),
                (80, NULL_CELL)
            ])
        );
    }

    /// 素材の途中から使うクリップ（イン点あり）。MotionClip の頭がタイムラインの
    /// クリップ頭に来る（C12 (1).clip: 尺を越えた 61〜105 コマに出ていた）
    #[test]
    fn clip_with_in_point() {
        let doc = general(vec![cel_clip_node(
            (35.0, 80.0),
            (26.0, 71.0),
            &[(0.0, "0"), (26.0, "1"), (28.0, "2"), (70.0, "24")],
        )]);
        assert_eq!(
            sheet_frames(&doc, 80),
            fr(&[
                (0, NULL_CELL),
                (35, "1"),
                (37, "2"),
                (79, "24"),
                (80, NULL_CELL)
            ])
        );
    }

    /// イン点より前で最後に出ていたセルが、クリップの頭から出る
    #[test]
    fn in_point_carries_previous_cel() {
        let doc = general(vec![cel_clip_node(
            (0.0, 24.0),
            (10.0, 34.0),
            &[(0.0, "1"), (5.0, "2"), (20.0, "3")],
        )]);
        assert_eq!(
            sheet_frames(&doc, 24),
            fr(&[(0, "2"), (10, "3"), (24, NULL_CELL)])
        );
    }

    /// 最初の露出が 0 コマ目より後なら、0 コマ目に空セルを置く
    /// （BRN_c013_si.clip / GED_c042_si_r.clip で抜けていた）
    #[test]
    fn leading_empty_cell() {
        let doc = general(vec![cel_clip_node(
            (0.0, 48.0),
            (0.0, 48.0),
            &[(0.0, ""), (12.0, "1"), (24.0, "2")],
        )]);
        assert_eq!(
            sheet_frames(&doc, 48),
            fr(&[(0, NULL_CELL), (12, "1"), (24, "2"), (48, NULL_CELL)])
        );
    }

    /// 尺を越えた変化点は捨てるが、クリップ終端の空セルは残す
    #[test]
    fn keys_past_duration_are_dropped() {
        let doc = general(vec![cel_clip_node(
            (0.0, 120.0),
            (0.0, 120.0),
            &[(0.0, "1"), (60.0, "2"), (63.0, "3")],
        )]);
        assert_eq!(
            sheet_frames(&doc, 62),
            fr(&[(0, "1"), (60, "2"), (120, NULL_CELL)])
        );
    }

    /// 0 コマ目より前（マイナス）の変化点は、最後のものだけ 0 コマ目へ持ち越す
    #[test]
    fn keys_before_start_carry_into_frame_zero() {
        let doc = general(vec![cel_clip_node(
            (-24.0, 24.0),
            (0.0, 48.0),
            &[(0.0, "1"), (6.0, "2"), (12.0, "3"), (30.0, "4")],
        )]);
        assert_eq!(
            sheet_frames(&doc, 24),
            fr(&[(0, "3"), (6, "4"), (24, NULL_CELL)])
        );
    }

    /// 2 本のクリップの間は空セル
    #[test]
    fn gap_between_clips_is_empty() {
        let doc = general(vec![
            cel_clip_node((0.0, 10.0), (0.0, 10.0), &[(0.0, "1")]),
            cel_clip_node((20.0, 30.0), (0.0, 10.0), &[(0.0, "2")]),
        ]);
        assert_eq!(
            sheet_frames(&doc, 30),
            fr(&[(0, "1"), (10, NULL_CELL), (20, "2"), (30, NULL_CELL)])
        );
    }

    #[test]
    fn overlapping_clips_are_rejected() {
        let doc = general(vec![
            cel_clip_node((0.0, 20.0), (0.0, 20.0), &[(0.0, "1")]),
            cel_clip_node((10.0, 30.0), (0.0, 20.0), &[(0.0, "2")]),
        ]);
        assert!(mixer_keys(&doc, 24.0, 0.0, 30.0).is_err());
    }
}
