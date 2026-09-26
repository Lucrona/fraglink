//! Fraglink core: everything the tray app does that is not UI.
//!
//! Read-only by design: the detector enumerates processes and reads Steam's
//! documented files/registry values. It never reads game memory, injects
//! code or installs hooks (anti-cheat safety). No network access.

pub mod detector;
pub mod engine;
pub mod env;
pub mod export;
pub mod signatures;
pub mod sim;
pub mod stats;
pub mod steam;
pub mod store;
pub mod tracker;
pub mod vdf;

pub use detector::{Detection, Detector};
pub use engine::Engine;
pub use store::Store;
pub use tracker::{Tracker, TrackerConfig, TrackerEvent};
