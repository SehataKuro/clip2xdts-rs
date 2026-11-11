//! clip2xdts の中核。CLIP STUDIO PAINT の `.clip` を読み、
//! タイムシートを XDTS / TDTS として書き出す。
//!
//! 形式そのものの読み書きは共有クレート [`xdts`](https://github.com/SehataKuro/xdts)
//! にある（依存はこれ 1 つだけで、その先は無い）。`.clip` の解析はここだけで行う。

pub mod clip;
mod cmt;
mod inflate;
pub mod sheet;
mod sqlite;

mod job;
pub use job::{collect_clips, Format, Job, convert_one, run_batch};
