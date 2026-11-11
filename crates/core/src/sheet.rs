//! タイムシートの書き出しと読み込み。中身は共有クレート [`xdts`] にあり、
//! ここはその薄い皮と、形式が変わっていないことを見張るテストだけ。

pub use xdts::{NULL_CELL, Sheet, TimeTable, Track};

/// 汎用デジタルタイムシート（XDTS、version 5）のテキスト
pub fn to_xdts(sheet: &Sheet) -> String {
    xdts::xdts::to_string(sheet, &Default::default())
}

/// 東映アニメーション デジタルタイムシート（TDTS、version 7）のテキスト
pub fn to_tdts(sheet: &Sheet) -> String {
    xdts::tdts::to_string(sheet, &Default::default())
}

/// どちらの形式でも、1 行目のマジック行を見て読む
pub fn from_str(text: &str) -> Result<Sheet, String> {
    xdts::from_str(text).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sheet() -> Sheet {
        Sheet {
            cut: "001".into(),
            tables: vec![TimeTable {
                name: "cut001".into(),
                duration: 24,
                columns: vec!["A".into()],
                tracks: vec![Track {
                    frames: vec![(0, "1".into()), (12, "2".into()), (24, NULL_CELL.into())],
                }],
                ..TimeTable::default()
            }],
            ..Sheet::default()
        }
    }

    /// 実機が保存したシートは version 10。version 7 は仕様書からの推測で、
    /// 実物が出てきて共有クレート側が直した
    #[test]
    fn magic_line_and_version() {
        let s = to_tdts(&sheet());
        assert!(s.starts_with("toeiDigitalTimeSheet Save Data\n"));
        assert!(s.contains("\"version\": 10"));
        assert!(s.contains("\"timeSheets\": ["));
    }

    #[test]
    fn xdts_magic_line_and_version() {
        let s = to_xdts(&sheet());
        assert!(s.starts_with("exchangeDigitalTimeSheet Save Data\n"));
        assert!(s.contains("\"version\": 5"));
        assert!(!s.contains("timeSheets"), "XDTS は timeSheets で包まない");
    }

    /// どちらの形式も LF。TDTS を CRLF で書いていたのは推測で、
    /// 実機が保存したシートは LF だった
    #[test]
    fn lf_everywhere() {
        assert!(
            !to_tdts(&sheet()).contains('\r'),
            "TDTS に CR が混ざっている"
        );
        assert!(
            !to_xdts(&sheet()).contains('\r'),
            "XDTS に CR が混ざっている"
        );
    }

    #[test]
    fn cells_carry_font_color_id() {
        let s = to_tdts(&sheet());
        // 東映 DTS は data 要素に fontColorId を持つ（XDTS には無い）
        assert!(s.contains("\"fontColorId\": 0"));
        assert!(!to_xdts(&sheet()).contains("fontColorId"));
        // セルの値は values の中。字下げの深さは見ない
        let flat = s.split_whitespace().collect::<Vec<_>>().join(" ");
        assert!(flat.contains("\"values\": [ \"1\" ]"));
        assert!(s.contains(NULL_CELL));
    }

    #[test]
    fn headers_cover_field_0_to_5() {
        let s = to_tdts(&sheet());
        for id in 0..=5 {
            assert!(
                s.contains(&format!("\"fieldId\": {}", id)),
                "fieldId {} が無い",
                id
            );
        }
        assert!(s.contains("\"S1\""), "台詞欄のヘッダが無い");
    }

    /// 1 枚のシートは 1 つの timeSheets 要素になる。
    /// 中の timeTables は「同じシートの改訂」なので、
    /// タイムラインごとに 1 枚ずつ書き出す今の作りでは常に 1 つ
    #[test]
    fn one_sheet_is_one_time_sheet() {
        let s = to_tdts(&sheet());
        assert_eq!(s.matches("\"timeTables\": [").count(), 1);
        // 名前は与えたものがそのまま残る（連番で潰さない）
        assert!(s.contains("\"name\": \"cut001\""));
    }

    /// 名前の無いシートだけ、東映 DTS に合わせて sheet1 と呼ぶ
    #[test]
    fn a_nameless_table_is_called_sheet1() {
        let mut x = sheet();
        x.tables[0].name = String::new();
        assert!(to_tdts(&x).contains("\"name\": \"sheet1\""));
    }

    #[test]
    fn work_slip_is_empty_but_present() {
        let s = to_tdts(&sheet());
        assert!(s.contains("\"workSlip\""));
        assert_eq!(s.matches("\"numberOfSheets\": 0").count(), 14); // 2 欄 × 7 トラック
        assert_eq!(s.matches("\"freeRole\"").count(), 3);
    }

    /// 書き出したものを読み返して、セルの中身が戻ってくること。
    /// 書き出し専用だった頃には書けなかったテスト
    #[test]
    fn a_sheet_survives_a_round_trip() {
        for text in [to_xdts(&sheet()), to_tdts(&sheet())] {
            let back = from_str(&text).unwrap();
            assert_eq!(back.cut, "001");
            assert_eq!(back.tables.len(), 1);
            let t = &back.tables[0];
            assert_eq!(t.duration, 24);
            assert_eq!(t.columns, vec!["A".to_string()]);
            assert_eq!(t.tracks, sheet().tables[0].tracks);
        }
    }

    /// 複数シート・空トラック・非 ASCII の欄名でも往復する
    #[test]
    fn several_sheets_survive_a_round_trip() {
        let mut x = sheet();
        x.scene = "12".into();
        x.tables.push(TimeTable {
            name: "cut002".into(),
            duration: 8,
            columns: vec!["背景".into(), "キャラ".into()],
            tracks: vec![
                Track {
                    frames: vec![(0, "1".into())],
                },
                Track::default(),
            ],
            ..TimeTable::default()
        });
        for text in [to_xdts(&x), to_tdts(&x)] {
            let back = from_str(&text).unwrap();
            assert_eq!(back.scene, "12");
            assert_eq!(back.tables.len(), 2);
            assert_eq!(
                back.tables[1].columns,
                vec!["背景".to_string(), "キャラ".to_string()]
            );
            assert_eq!(back.tables[1].tracks, x.tables[1].tracks);
            // name は TDTS では sheetN に振り直されるので、比較はセルだけ
            assert_eq!(back.tables[0].tracks, x.tables[0].tracks);
        }
    }

    /// 読めないものは読めないと言う
    #[test]
    fn something_else_is_not_a_sheet() {
        assert!(from_str("hello\n{}").is_err());
    }
}
