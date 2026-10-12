//! The app's fixed local ports, in one place, all shifted by one switch:
//! `YOAI_PORT_OFFSET=<n>` adds n to every one of them, so a second scratch
//! instance (the two-device harness, two profiles on one dev box) runs
//! beside the first instead of fighting it for 8080 / 11435 / 4477. Zero
//! (the default, and every normal install) leaves the ports exactly as they
//! have always been. Read once; never a per-port switch.

use std::sync::OnceLock;

fn offset() -> u16 {
    static OFFSET: OnceLock<u16> = OnceLock::new();
    *OFFSET.get_or_init(|| {
        std::env::var("YOAI_PORT_OFFSET")
            .ok()
            .and_then(|v| v.trim().parse::<u16>().ok())
            .filter(|n| *n <= 20_000)
            .unwrap_or(0)
    })
}

/// The chat engine (llama-server for the loaded model).
pub fn chat() -> u16 { 8080 + offset() }
/// The memory component's embedding server.
pub fn embed() -> u16 { 8091 + offset() }
/// The helper (utility) model's server.
pub fn util() -> u16 { 8092 + offset() }
/// The inference provider and MCP endpoint (also the single-instance lock).
pub fn inference() -> u16 { 11435 + offset() }
/// The conductor's admin websocket.
pub fn admin_ws() -> u16 { 4477 + offset() }
/// The truth matrix / tune bench server.
pub fn bench() -> u16 { 18098 + offset() }

#[cfg(test)]
mod tests {
    #[test]
    fn defaults_are_the_ports_the_app_has_always_used() {
        // The switch is read once per process; without it every port is the historic one.
        if std::env::var("YOAI_PORT_OFFSET").is_err() {
            assert_eq!(super::chat(), 8080);
            assert_eq!(super::embed(), 8091);
            assert_eq!(super::util(), 8092);
            assert_eq!(super::inference(), 11435);
            assert_eq!(super::admin_ws(), 4477);
            assert_eq!(super::bench(), 18098);
        }
    }
}
