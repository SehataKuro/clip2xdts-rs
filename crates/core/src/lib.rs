//! clip2xdts の中核。CLIP STUDIO PAINT の `.clip` を読み、
//! タイムシートを XDTS / TDTS として書き出す。外部 crate には依存しない。

pub mod clip;
mod cmt;
mod inflate;
mod sqlite;
pub mod tdts;
pub mod xdts;

mod job;
pub use job::{collect_clips, Format, Job, convert_one, run_batch};
