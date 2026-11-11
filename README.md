# clip2xdts

CLIP STUDIO PAINT の `.clip` から、タイムシート `.xdts` / `.tdts` をまとめて書き出します。
クリスタ本体は不要で、PRO 版のファイルでも変換できます。CLI と GUI の両方があります。

## ビルド

    cargo build --release

できるもの:

- `target/release/clip2xdts`（CLI）
- `target/release/clip2xdts-gui`（GUI）

GUI だけ要らない場合は `cargo build --release -p clip2xdts` で CLI だけ作れます。

## GUI

    clip2xdts-gui

`.clip` ファイルかフォルダを窓に落として、形式を選んで「変換」を押すだけです。
フォルダはサブフォルダまで辿ります。出力先は既定で `.clip` と同じ場所、
ボタンから別のフォルダも選べます。日本語の表示には OS に入っているフォント
（Windows なら游ゴシック→メイリオ→MS ゴシックの順）を使います。

## CLI

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
| `.tdts` | 東映アニメーション デジタルタイムシート version 10 |

読み込みにも対応していて、書き出した `.xdts` / `.tdts` をそのまま読み返せます。

`.tdts` は `.xdts` と同系統の JSON（1 行目にマジック行）ですが、字下げは 4 スペースで、全体を
`timeSheets[]` で包み、セルに `fontColorId` が付き、文字色表・作業伝票の枠を持ちます。
`.clip` には作業伝票・作業者名・話数の情報が無いので、そこは東映 DTS の既定値（空）で埋めます。
改行はどちらの形式も LF です。以前は `.tdts` を version 7・CRLF で書いていましたが、
これは仕様書からの推測でした。東映 DTS が実際に保存したシートが見つかり、version 10・LF・
字下げ 4 スペースだと分かったので、共有クレート側で直っています。

## 出力の仕様（CLIP STUDIO PAINT EX の標準書き出しに合わせています）

- 親フォルダも含めて表示中のアニメーションフォルダだけを、レイヤーパレットの順に並べる
- 対象は選択中のタイムライン（`--all-timelines` で全タイムライン）
- タイムライン 1 本につき 1 ファイル。複数書き出すときは `a_sheet1.xdts` `a_sheet2.xdts` … と連番を付け、
  1 本だけのときは `a.xdts` のまま
- 最後のセルのあと、クリップの終端に空セル（SYMBOL_NULL_CELL）を置く

## 未対応

- 中割り・逆シートの記号、カメラフォルダ、サウンド欄（S0〜S9）
- `.tdts` の台詞欄（fieldId 3）・カメラ欄（fieldId 5）は空のまま出力する
- TDTS は公式仕様が非公開。共有クレートが東映 DTS の保存したシートに合わせて書くが、
  東映 DTS 実機での読み込みは未検証
- 途中に入る空セルの判定は推測による実装（未検証）

## 実ファイルでの確認

1 カット（タイムライン 1 本、セル欄 2 列）で、クリスタ EX が書き出した `.xdts` と
本ツールの出力が **バイト単位で完全に一致**した（どちらも 5,393 バイト、SHA-256 も同一）。
キーの並びや字下げまで同じものが出ます。

`.tdts` のほうは、共有クレートが東映 DTS の保存したシートに合わせて書きますが、
東映 DTS 実機での読み込みは未検証です。

## 構成

3 つの crate のワークスペースです。変換の中身は `core` だけにあり、CLI と GUI は
その薄い皮です。XDTS / TDTS 自体の読み書きは共有クレート
[xdts](https://crates.io/crates/xdts)（[SehataKuro/xdts](https://github.com/SehataKuro/xdts)）
にあり、`core` の依存はこれ 1 つだけ
（xdts 側は依存ゼロなので、依存ツリーはこれで終わり）。GUI の依存（egui / rfd）は
`gui` の中だけに閉じています。

| crate | 内容 |
|---|---|
| `crates/core` | .clip の読み取りと XDTS / TDTS の組み立て。依存は `xdts` のみ |
| `crates/cli`  | `clip2xdts` コマンド |
| `crates/gui`  | `clip2xdts-gui`（egui / rfd） |

`crates/core` の中身:

| ファイル | 内容 |
|---|---|
| `clip.rs`    | .clip のチャンク読み取り、タイムラインから XDTS への変換 |
| `sqlite.rs`  | 読み取り専用の最小 SQLite リーダー |
| `inflate.rs` | zlib 展開 |
| `cmt.rs`     | TrackActionMixer（`cmt 0110 binc` 形式）のパーサー |
| `sheet.rs`   | 共有クレート `xdts` への薄い皮（書き出し・読み込み）と形式のテスト |
| `job.rs`     | 入力の収集と、ファイル単位の並列変換（CLI と GUI が共用） |
