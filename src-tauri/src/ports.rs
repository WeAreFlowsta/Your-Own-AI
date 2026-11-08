//! The app's fixed local ports, in one place, all shifted by one switch:
//! `YOAI_PORT_OFFSET=<n>` adds n to every one of them, so a second scratch
//! instance (the two-device harness, two profiles on one dev box) runs
//! beside the first instead of fighting it for 8080 / 11435 / 4477. Zero
//! (the default, and every normal install) leaves the ports exactly as they
//! have always been. Read once; never a per-port switch.

use std::sync::atomic::{AtomicU16, Ordering};
use std::sync::OnceLock;

pub fn offset() -> u16 {
    static OFFSET: OnceLock<u16> = OnceLock::new();
    *OFFSET.get_or_init(|| {
        std::env::var("YOAI_PORT_OFFSET")
            .ok()
            .and_then(|v| v.trim().parse::<u16>().ok())
            .filter(|n| *n <= 20_000)
            .unwrap_or(0)
    })
}

/// The three engine servers. Each starts on its usual port; when another
/// program holds that port, the server moves to a free one for the rest of
/// the session (`llm::claim_engine_port`) and every caller follows, since
/// they all ask here each time.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Engine {
    Chat,
    Embed,
    Util,
}

static CHAT: AtomicU16 = AtomicU16::new(0);
static EMBED: AtomicU16 = AtomicU16::new(0);
static UTIL: AtomicU16 = AtomicU16::new(0);

fn slot(engine: Engine) -> (&'static AtomicU16, u16) {
    match engine {
        Engine::Chat => (&CHAT, 8080),
        Engine::Embed => (&EMBED, 8091),
        Engine::Util => (&UTIL, 8092),
    }
}

/// The port `engine` uses this session.
pub fn engine(engine: Engine) -> u16 {
    let (cell, usual) = slot(engine);
    match cell.load(Ordering::Relaxed) {
        0 => usual + offset(),
        moved => moved,
    }
}

/// `engine` serves on `port` for the rest of the session.
pub(crate) fn move_engine(engine: Engine, port: u16) {
    slot(engine).0.store(port, Ordering::Relaxed);
}

/// The chat engine (llama-server, or SwiftLM, for the loaded model).
pub fn chat() -> u16 { engine(Engine::Chat) }
/// The memory component's embedding server.
pub fn embed() -> u16 { engine(Engine::Embed) }
/// The helper (utility) model's server.
pub fn util() -> u16 { engine(Engine::Util) }

/// Whether a server could listen on `port` on loopback right now.
pub(crate) fn bindable(port: u16) -> bool {
    std::net::TcpListener::bind(("127.0.0.1", port)).is_ok()
}

/// A loopback port the OS reports free right now.
pub(crate) fn free_port() -> Option<u16> {
    let listener = std::net::TcpListener::bind(("127.0.0.1", 0)).ok()?;
    listener.local_addr().ok().map(|a| a.port())
}
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

    #[test]
    fn a_held_port_is_not_bindable_and_a_free_one_is_found() {
        let held = std::net::TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let port = held.local_addr().unwrap().port();
        assert!(!super::bindable(port), "a port another listener holds");
        // A port free_port reports was free when it asked; whether it still
        // is a moment later depends on the other tests' listeners, so only
        // that it is another port is checked.
        let free = super::free_port().expect("the OS has a free port");
        assert_ne!(free, port);
        // Not checked: that the port is bindable again once dropped. Other
        // tests bind port 0 in parallel, and the OS may hand them this one
        // the moment it is free.
        drop(held);
    }
}
