//! Integrazione con la build custom di ffmpeg (SVT-AV1, libopus, dav1d, libvmaf)
//! definita in `Dockerfile.ffmpeg-svtav1`.

pub mod encode;
pub mod probe;
pub mod progress;

pub use encode::{FfmpegEvent, RunOutcome};
pub use probe::MediaInfo;
pub use progress::{format_progress_line, ProgressParser};
