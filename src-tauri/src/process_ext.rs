//! Spawning the sidecar processes (lair-keystore, the holochain conductor)
//! tied to the app's lifetime and without a console window, and stopping
//! them cleanly. The code lives in `flowsta-hc-lines`; the app names its
//! sidecars for the Windows console sweep at startup (`lib.rs`).

pub use flowsta_hc_lines::process::*;
