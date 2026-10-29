//! Small custom-drawn pieces of the UI. Each one paints inside the rect it
//! allocates (clipped), so an oversized value can never spill out of its row.

mod cores;
mod metrics;
mod procs;
mod sparkline;
mod text;

pub use cores::core_bars;
pub use metrics::{kv_row, metric_row, section_title};
pub use procs::ProcTable;
pub use sparkline::sparkline;
