//! TDTS（東映アニメーション デジタルタイムシート）の書き出し
//!
//! XDTS と同じ入れ子の JSON だが、`timeSheets[]` で包み `version` は 7。
//! セル欄（fieldId 0）以外の欄と作業伝票は .clip に情報が無いので空で埋める。

use crate::xdts::{write, Xdts, J};

/// 東映 DTS の既定の文字色表（黒・赤・緑・青・紫・橙）
const FONT_COLORS: [[i64; 3]; 6] = [
    [0, 0, 0],
    [224, 0, 0],
    [32, 128, 32],
    [32, 32, 192],
    [192, 32, 192],
    [255, 128, 32],
];

fn empty_work_slip() -> J {
    let process = || {
        J::Obj(vec![
            ("assistant", J::Str(String::new())),
            ("chief", J::Str(String::new())),
            ("director", J::Str(String::new())),
            ("free", J::Str(String::new())),
            ("freeRole", J::Str(String::new())),
        ])
    };
    let work = || {
        let tracks = (0..7)
            .map(|_| {
                J::Obj(vec![
                    ("artist", J::Str(String::new())),
                    ("numberOfSheets", J::Int(0)),
                ])
            })
            .collect();
        J::Obj(vec![("enable", J::Bool(false)), ("tracks", J::Arr(tracks))])
    };
    J::Obj(vec![
        ("memo", J::Str(String::new())),
        ("processes", J::Arr((0..3).map(|_| process()).collect())),
        ("works", J::Arr((0..2).map(|_| work()).collect())),
    ])
}

/// fieldId 0〜5 のヘッダ。セル欄だけ実データで、残りは東映 DTS の既定値
fn headers(names: &[String]) -> J {
    let strs = |v: &[&str]| J::Arr(v.iter().map(|s| J::Str(s.to_string())).collect());
    J::Arr(vec![
        J::Obj(vec![
            ("fieldId", J::Int(0)),
            ("names", J::Arr(names.iter().map(|n| J::Str(n.clone())).collect())),
        ]),
        J::Obj(vec![("fieldId", J::Int(1)), ("names", strs(&[""]))]),
        J::Obj(vec![("fieldId", J::Int(2)), ("names", strs(&[""]))]),
        J::Obj(vec![("fieldId", J::Int(3)), ("names", strs(&["S1", "S2"]))]),
        J::Obj(vec![("fieldId", J::Int(4)), ("names", J::Arr(vec![]))]),
        J::Obj(vec![("fieldId", J::Int(5)), ("names", strs(&["1", "2"]))]),
    ])
}

/// XDTS 用に組んだシートから TDTS のテキストを作る
pub fn to_string(x: &Xdts) -> String {
    let tables = x
        .tables
        .iter()
        .enumerate()
        .map(|(i, t)| {
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
                                (
                                    "data",
                                    J::Arr(vec![J::Obj(vec![
                                        ("fontColorId", J::Int(0)),
                                        ("id", J::Int(0)),
                                        ("values", J::Arr(vec![J::Str(f.value.clone())])),
                                    ])]),
                                ),
                                ("frame", J::Int(f.frame)),
                            ])
                        })
                        .collect();
                    J::Obj(vec![("frames", J::Arr(frames)), ("trackNo", J::Int(no as i64))])
                })
                .collect();
            J::Obj(vec![
                (
                    "books",
                    J::Arr(vec![J::Obj(vec![
                        ("fieldId", J::Int(0)),
                        ("tracks", J::Arr(vec![])),
                    ])]),
                ),
                ("color", J::Int(0)),
                ("direction", J::Str(String::new())),
                ("duration", J::Int(t.duration)),
                (
                    "fields",
                    J::Arr(vec![J::Obj(vec![
                        ("fieldId", J::Int(0)),
                        ("tracks", J::Arr(tracks)),
                    ])]),
                ),
                ("name", J::Str(format!("sheet{}", i + 1))),
                ("operatorName", J::Str(String::new())),
                ("timeTableHeaders", headers(&t.names)),
            ])
        })
        .collect();
    let colors = FONT_COLORS
        .iter()
        .map(|c| J::Arr(c.iter().map(|v| J::Int(*v)).collect()))
        .collect();
    let sheet = J::Obj(vec![
        (
            "header",
            J::Obj(vec![
                ("cut", J::Str(x.cut.clone())),
                ("episode", J::Str(String::new())),
                ("scene", J::Str(x.scene.clone())),
                ("showHeaderDummy", J::Bool(false)),
                ("timeTableFontColors", J::Arr(colors)),
                ("workSlip", empty_work_slip()),
            ]),
        ),
        ("timeTables", J::Arr(tables)),
    ]);
    let root = J::Obj(vec![
        ("timeSheets", J::Arr(vec![sheet])),
        ("version", J::Int(7)),
    ]);
    let mut s = String::from("toeiDigitalTimeSheet Save Data\n");
    write(&root, 0, &mut s);
    s.push('\n');
    // 東映 DTS の保存ファイルは CRLF
    s.replace('\n', "\r\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::xdts::{Frame, TimeTable, Track, NULL_CELL};

    fn sheet() -> Xdts {
        Xdts {
            cut: "001".into(),
            scene: "".into(),
            tables: vec![TimeTable {
                name: "cut001".into(),
                duration: 24,
                names: vec!["A".into()],
                tracks: vec![Track {
                    frames: vec![
                        Frame { frame: 0, value: "1".into() },
                        Frame { frame: 12, value: "2".into() },
                        Frame { frame: 24, value: NULL_CELL.into() },
                    ],
                }],
            }],
        }
    }

    #[test]
    fn magic_line_and_version() {
        let s = to_string(&sheet());
        assert!(s.starts_with("toeiDigitalTimeSheet Save Data\r\n"));
        assert!(s.contains("\"version\": 7"));
        assert!(s.contains("\"timeSheets\": ["));
    }

    #[test]
    fn crlf_everywhere() {
        let s = to_string(&sheet());
        assert!(!s.replace("\r\n", "").contains('\n'), "LF が残っている");
    }

    #[test]
    fn cells_carry_font_color_id() {
        let s = to_string(&sheet());
        // 東映 DTS は data 要素に fontColorId を持つ（XDTS には無い）
        assert!(s.contains("\"fontColorId\": 0"));
        assert!(s.contains("\"values\": [\r\n                            \"1\"\r\n                          ]"));
        assert!(s.contains(NULL_CELL));
    }

    #[test]
    fn headers_cover_field_0_to_5() {
        let s = to_string(&sheet());
        for id in 0..=5 {
            assert!(s.contains(&format!("\"fieldId\": {}", id)), "fieldId {} が無い", id);
        }
        assert!(s.contains("\"S1\""), "台詞欄のヘッダが無い");
    }

    #[test]
    fn sheet_name_is_sequential() {
        let mut x = sheet();
        x.tables.push(TimeTable {
            name: "cut002".into(),
            duration: 12,
            names: vec!["A".into()],
            tracks: vec![Track { frames: vec![] }],
        });
        let s = to_string(&x);
        assert!(s.contains("\"name\": \"sheet1\""));
        assert!(s.contains("\"name\": \"sheet2\""));
    }

    #[test]
    fn work_slip_is_empty_but_present() {
        let s = to_string(&sheet());
        assert!(s.contains("\"workSlip\""));
        assert_eq!(s.matches("\"numberOfSheets\": 0").count(), 14); // 2 欄 × 7 トラック
        assert_eq!(s.matches("\"freeRole\"").count(), 3);
    }
}
