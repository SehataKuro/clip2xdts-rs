# clip2xdts

CLIP STUDIO PAINT の `.clip` から、タイムシート `.xdts` をまとめて書き出す CLI です。
クリスタ本体は不要で、PRO 版のファイルでも変換できます。外部 crate への依存はありません。

## ビルド

    cargo build --release

## 使い方

    clip2xdts カット/*.clip            # 各 .clip の隣に .xdts を作る
    clip2xdts -o xdts/ カット/          # フォルダ内（サブフォルダ含む）の .clip を一括変換
    clip2xdts --all-timelines a.clip   # 全タイムラインを書き出す
    clip2xdts -j 4 ...                 # 同時に処理するファイル数を指定

## 出力の仕様（CLIP STUDIO PAINT EX の標準書き出しに合わせています）

- 親フォルダも含めて表示中のアニメーションフォルダだけを、レイヤーパレットの順に並べる
- 対象は選択中のタイムライン（`--all-timelines` で全タイムライン）
- 最後のセルのあと、クリップの終端に空セル（SYMBOL_NULL_CELL）を置く

## 未対応

- 中割り・逆シートの記号、カメラフォルダ、サウンド欄（S0〜S9）
- 途中に入る空セルの判定は推測による実装（未検証）

## 構成

| ファイル | 内容 |
|---|---|
| `src/clip.rs`    | .clip のチャンク読み取り、タイムラインから XDTS への変換 |
| `src/sqlite.rs`  | 読み取り専用の最小 SQLite リーダー |
| `src/inflate.rs` | zlib 展開 |
| `src/cmt.rs`     | TrackActionMixer（`cmt 0110 binc` 形式）のパーサー |
| `src/xdts.rs`    | XDTS の JSON 出力 |
