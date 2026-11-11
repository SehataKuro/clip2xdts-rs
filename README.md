# clip2xdts

CLIP STUDIO PAINT の `.clip` から、タイムシート `.xdts` / `.tdts` をまとめて書き出す CLI です。
クリスタ本体は不要で、PRO 版のファイルでも変換できます。外部 crate への依存はありません。

## ビルド

    cargo build --release

## 使い方

    clip2xdts カット/*.clip            # 各 .clip の隣に .xdts を作る
    clip2xdts -o xdts/ カット/          # フォルダ内（サブフォルダ含む）の .clip を一括変換
    clip2xdts -f tdts カット/*.clip     # 東映 DTS 形式 (.tdts) で書き出す
    clip2xdts -f both カット/*.clip     # 両方の形式を書き出す
    clip2xdts --all-timelines a.clip   # 全タイムラインを書き出す
    clip2xdts -j 4 ...                 # 同時に処理するファイル数を指定

## 出力形式

| 形式 | 中身 |
|---|---|
| `.xdts` | 汎用デジタルタイムシート version 5。CLIP STUDIO PAINT EX の標準書き出しと同じ |
| `.tdts` | 東映アニメーション デジタルタイムシート version 7 |

`.tdts` は `.xdts` と同系統の JSON（1 行目にマジック行、以降 2 スペース整形）ですが、全体を
`timeSheets[]` で包み、セルに `fontColorId` が付き、文字色表・作業伝票の枠を持ちます。
`.clip` には作業伝票・作業者名・話数の情報が無いので、そこは東映 DTS の既定値（空）で埋めます。
改行は東映 DTS の保存ファイルに合わせて CRLF です。

## 出力の仕様（CLIP STUDIO PAINT EX の標準書き出しに合わせています）

- 親フォルダも含めて表示中のアニメーションフォルダだけを、レイヤーパレットの順に並べる
- 対象は選択中のタイムライン（`--all-timelines` で全タイムライン）
- 最後のセルのあと、クリップの終端に空セル（SYMBOL_NULL_CELL）を置く

## 未対応

- 中割り・逆シートの記号、カメラフォルダ、サウンド欄（S0〜S9）
- `.tdts` の台詞欄（fieldId 3）・カメラ欄（fieldId 5）は空のまま出力する
- TDTS は公式仕様が非公開で、[lunafuse/vdts-editor](https://github.com/lunafuse/vdts-editor) の
  仕様書とサンプルから構造を起こしている。東映 DTS 実機での読み込みは未検証
- 途中に入る空セルの判定は推測による実装（未検証）

## 構成

| ファイル | 内容 |
|---|---|
| `src/clip.rs`    | .clip のチャンク読み取り、タイムラインから XDTS への変換 |
| `src/sqlite.rs`  | 読み取り専用の最小 SQLite リーダー |
| `src/inflate.rs` | zlib 展開 |
| `src/cmt.rs`     | TrackActionMixer（`cmt 0110 binc` 形式）のパーサー |
| `src/xdts.rs`    | XDTS の JSON 出力と、共用の JSON ライター |
| `src/tdts.rs`    | TDTS の JSON 出力 |
