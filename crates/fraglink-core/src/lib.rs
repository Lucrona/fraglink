//! Fraglink core: everything the tray app does that is not UI.
//!
//! Read-only by design: the detector enumerates processes and reads Steam's
//! documented files/registry values (Steam, Epic, GOG). It never reads game memory, injects
//! code or installs hooks (anti-cheat safety). No network access (the desktop app
//! fetches the game lists and hands them to the [`Detector`]).

pub mod catalog;
pub mod detectable;
pub mod detector;
pub mod engine;
pub mod env;
pub mod export;
pub mod folderscan;
pub mod icons;
pub mod installs;
pub mod libraries;
pub mod overrides;
pub mod realm;
pub mod signatures;
pub mod sim;
pub mod stats;
pub mod steam;
pub mod store;
pub mod tracker;
pub mod vdf;
pub mod wow_played;

pub use detector::{Detection, Detector};
pub use engine::Engine;
pub use store::Store;
pub use tracker::{Tracker, TrackerConfig, TrackerEvent};
