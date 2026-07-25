//! av1webconverter — backend di conversione video AV1 con frontend embedded.
//!
//! La catena di codifica riproduce gli script bash di riferimento
//! (`start_av1_docker.sh`, `execute_docker.sh`) usando la build custom di ffmpeg
//! con SVT-AV1 descritta in `Dockerfile.ffmpeg-svtav1`, ma eseguendo ffmpeg
//! nello stesso container invece che via SSH.

pub mod api;
pub mod auth;
pub mod config;
pub mod error;
pub mod events;
pub mod ffmpeg;
pub mod joblog;
pub mod logging;
pub mod models;
pub mod slack;
pub mod state;
pub mod store;
pub mod worker;
