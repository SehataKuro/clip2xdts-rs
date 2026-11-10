//! XDTS のデータ構造と書き出し（クリスタと同じ 2 スペース整形の JSON）

pub const NULL_CELL: &str = "SYMBOL_NULL_CELL";

pub struct Frame {
    pub frame: i64,
    pub value: String,
}

pub struct Track {
    pub frames: Vec<Frame>,
}

pub struct TimeTable {
    pub name: String,
    pub duration: i64,
    pub names: Vec<String>,
    pub tracks: Vec<Track>,
}

pub struct Xdts {
    pub cut: String,
    pub scene: String,
    pub tables: Vec<TimeTable>,
}

/// 小さな JSON 木。Python の json.dumps(indent=2, ensure_ascii=False) と同じ見た目で出す
enum J {
    Int(i64),
    Str(String),
    Arr(Vec<J>),
    Obj(Vec<(&'static str, J)>),
}

fn esc(s: &str, out: &mut String) {
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\u{8}' => out.push_str("\\b"),
            '\u{c}' => out.push_str("\\f"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
}

fn write(j: &J, ind: usize, out: &mut String) {
    let pad = |n: usize, out: &mut String| out.extend(std::iter::repeat(' ').take(n));
    match j {
        J::Int(v) => out.push_str(&v.to_string()),
        J::Str(s) => esc(s, out),
        J::Arr(a) if a.is_empty() => out.push_str("[]"),
        J::Obj(o) if o.is_empty() => out.push_str("{}"),
        J::Arr(a) => {
            out.push_str("[\n");
            for (i, v) in a.iter().enumerate() {
                pad(ind + 2, out);
                write(v, ind + 2, out);
                out.push_str(if i + 1 < a.len() { ",\n" } else { "\n" });
            }
            pad(ind, out);
            out.push(']');
        }
        J::Obj(o) => {
            out.push_str("{\n");
            for (i, (k, v)) in o.iter().enumerate() {
                pad(ind + 2, out);
                esc(k, out);
                out.push_str(": ");
                write(v, ind + 2, out);
                out.push_str(if i + 1 < o.len() { ",\n" } else { "\n" });
            }
            pad(ind, out);
            out.push('}');
        }
    }
}

impl Xdts {
    pub fn to_string(&self) -> String {
        let tables = self
            .tables
            .iter()
            .map(|t| {
                let tracks = t
                    .tracks
                    .iter()
                    .enumerate()
                    .map(|(no, tr)| {
                        let frames = tr
                            .frames
                            .iter()
                            .map(|f| {
                                J::Obj(vec![
                                    ("frame", J::Int(f.frame)),
                                    (
                                        "data",
                                        J::Arr(vec![J::Obj(vec![
                                            ("id", J::Int(0)),
                                            ("values", J::Arr(vec![J::Str(f.value.clone())])),
                                        ])]),
                                    ),
                                ])
                            })
                            .collect();
                        J::Obj(vec![("trackNo", J::Int(no as i64)), ("frames", J::Arr(frames))])
                    })
                    .collect();
                J::Obj(vec![
                    ("name", J::Str(t.name.clone())),
                    ("duration", J::Int(t.duration)),
                    (
                        "timeTableHeaders",
                        J::Arr(vec![J::Obj(vec![
                            ("fieldId", J::Int(0)),
                            ("names", J::Arr(t.names.iter().map(|n| J::Str(n.clone())).collect())),
                        ])]),
                    ),
                    (
                        "fields",
                        J::Arr(vec![J::Obj(vec![("fieldId", J::Int(0)), ("tracks", J::Arr(tracks))])]),
                    ),
                ])
            })
            .collect();
        let root = J::Obj(vec![
            ("version", J::Int(5)),
            (
                "header",
                J::Obj(vec![("cut", J::Str(self.cut.clone())), ("scene", J::Str(self.scene.clone()))]),
            ),
            ("timeTables", J::Arr(tables)),
        ]);
        let mut s = String::from("exchangeDigitalTimeSheet Save Data\n");
        write(&root, 0, &mut s);
        s.push('\n');
        s
    }
}
