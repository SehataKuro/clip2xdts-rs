//! タイムシートの書き出しと読み込み。中身は共有クレート [`xdts`] にあり、
//! ここはその薄い皮と、形式が変わっていないことを見張るテストだけ。

pub use xdts::{Sheet, TimeTable, Track, NULL_CELL};

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

    #[test]
    fn magic_line_and_version() {
        let s = to_tdts(&sheet());
        assert!(s.starts_with("toeiDigitalTimeSheet Save Data\r\n"));
        assert!(s.contains("\"version\": 7"));
        assert!(s.contains("\"timeSheets\": ["));
    }

    #[test]
    fn xdts_magic_line_and_version() {
        let s = to_xdts(&sheet());
        assert!(s.starts_with("exchangeDigitalTimeSheet Save Data\n"));
        assert!(s.contains("\"version\": 5"));
        assert!(!s.contains("timeSheets"), "XDTS は timeSheets で包まない");
    }

    #[test]
    fn crlf_everywhere() {
        let s = to_tdts(&sheet());
        assert!(!s.replace("\r\n", "").contains('\n'), "LF が残っている");
        assert!(!to_xdts(&sheet()).contains('\r'), "XDTS に CR が混ざっている");
    }

    #[test]
    fn cells_carry_font_color_id() {
        let s = to_tdts(&sheet());
        // 東映 DTS は data 要素に fontColorId を持つ（XDTS には無い）
        assert!(s.contains("\"fontColorId\": 0"));
        assert!(!to_xdts(&sheet()).contains("fontColorId"));
        assert!(s.contains("\"values\": [\r\n                            \"1\"\r\n                          ]"));
        assert!(s.contains(NULL_CELL));
    }

    #[test]
    fn headers_cover_field_0_to_5() {
        let s = to_tdts(&sheet());
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
            columns: vec!["A".into()],
            tracks: vec![Track::default()],
            ..TimeTable::default()
        });
        let s = to_tdts(&x);
        assert!(s.contains("\"name\": \"sheet1\""));
        assert!(s.contains("\"name\": \"sheet2\""));
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
            tracks: vec![Track { frames: vec![(0, "1".into())] }, Track::default()],
            ..TimeTable::default()
        });
        for text in [to_xdts(&x), to_tdts(&x)] {
            let back = from_str(&text).unwrap();
            assert_eq!(back.scene, "12");
            assert_eq!(back.tables.len(), 2);
            assert_eq!(back.tables[1].columns, vec!["背景".to_string(), "キャラ".to_string()]);
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
