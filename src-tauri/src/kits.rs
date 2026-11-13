//! The kit cache (flowsta-hc-lines `kit`): once a run, after a line is up,
//! each Holochain line this release bundles (its conductor, its key store
//! and the files it loads) is set aside compressed in `<app data>/kits/`.
//!
//! It is filled now for a later release. An installer replaces the app's
//! folder, sidecars included, and keeps the data folder; a release that no
//! longer bundles a line reads a device's own records on that line from the
//! kit cached here, offline, after checking every file against the hashes
//! it compiles in. Only a release that still bundles a line can cache it.

use flowsta_hc_lines::kit::{KitBinary, KitCache, KitDescriptor, KitFile};
use flowsta_hc_lines::Line;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

/// The 0.6.1 line's kit. `accepted` stays empty while a release bundles the
/// binaries: their post-signing hashes come out of this release's build and
/// go into the next one.
pub const KIT_06: KitDescriptor = KitDescriptor {
    line: Line::V06,
    conductor: KitBinary { sidecar: "yourowai-holochain", version: "0.6.1", accepted: &[] },
    lair: KitBinary { sidecar: "yourowai-lair-keystore", version: "0.6.3", accepted: &[] },
    files: &[
        KitFile { name: "yourown_ai_transcript_v1_happ.happ", sha256: "d430328dbdaf5578e8d849a3a4e61808ecc9a68e88f98485b1cf17013b33436f" },
        // The coordinator the v1 cells run (dna.rs COORDINATOR_VERSION): a
        // new one changes this hash, and the test below says so.
        KitFile { name: "transcript_coordinator.wasm", sha256: "ccb3dba9710ee68807d70eb222b024b5ab3bec3828162989f9a20ffdd7057857" },
    ],
};

/// The 0.7.1 line's kit.
pub const KIT_07: KitDescriptor = KitDescriptor {
    line: Line::V07,
    conductor: KitBinary { sidecar: "yourowai-holochain-07", version: "0.7.1", accepted: &[] },
    lair: KitBinary { sidecar: "yourowai-lair-keystore-07", version: "0.7.1", accepted: &[] },
    files: &[KitFile { name: crate::hc07::HAPP_FILENAME, sha256: crate::hc07::HAPP_SHA256 }],
};

/// Where the Holochain sidecars are: next to the app, or `src-tauri/binaries`
/// in a debug build.
fn sidecar_source() -> flowsta_hc_lines::SidecarSource {
    flowsta_hc_lines::SidecarSource {
        dev_dir: cfg!(debug_assertions).then(|| Path::new(env!("CARGO_MANIFEST_DIR")).join("binaries")),
        exe_dir: None,
        kit_dirs: Vec::new(),
        allow_path_lookup: false,
    }
}

pub fn kits_dir(device_root: &Path) -> PathBuf {
    device_root.join("kits")
}

static REFRESHED: AtomicBool = AtomicBool::new(false);

/// `refresh`, once a run, on a blocking thread.
pub fn refresh_once(device_root: PathBuf, resources: PathBuf) {
    if REFRESHED.swap(true, Ordering::SeqCst) {
        return;
    }
    tauri::async_runtime::spawn_blocking(move || refresh(&device_root, &resources));
}

/// Set both lines' kits aside. A file already cached as it is costs one
/// read to hash it; nothing here can stop a start.
pub fn refresh(device_root: &Path, resources: &Path) {
    let cache = KitCache::new(kits_dir(device_root));
    let sidecars = sidecar_source();
    for kit in [&KIT_07, &KIT_06] {
        match cache.put(kit, &sidecars, resources) {
            Ok(p) if p.written.is_empty() => log::info!("[kit] the {} kit is cached as it is ({} KB)", p.line, p.packed / 1024),
            Ok(p) => log::info!("[kit] the {} kit cached: {} written, {} as before ({} KB)", p.line, p.written.join(", "), p.kept, p.packed / 1024),
            Err(e) => log::warn!("[kit] the {} kit not cached: {}", kit.line, e),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sha2::{Digest, Sha256};

    /// Every file a kit names is the one in `resources/`: a new bundle or
    /// coordinator must come with its hash here, or no kit would be cached
    /// (`put` refuses a file that is not the one described).
    #[test]
    fn the_kits_name_the_files_this_release_bundles() {
        let resources = Path::new(env!("CARGO_MANIFEST_DIR")).join("resources");
        for kit in [&KIT_06, &KIT_07] {
            for f in kit.files {
                let bytes = std::fs::read(resources.join(f.name)).unwrap_or_else(|e| panic!("{}: {e}", f.name));
                assert_eq!(hex::encode(Sha256::digest(&bytes)), f.sha256, "{} changed: update its hash in kits.rs", f.name);
            }
        }
    }

    /// Both kits go into a cache from this machine's sidecars and the
    /// bundled files; a second pass writes nothing.
    #[test]
    fn both_kits_are_cached_and_a_second_pass_writes_nothing() {
        let binaries = Path::new(env!("CARGO_MANIFEST_DIR")).join("binaries");
        let have = |name: &str| binaries.join(format!("{}-{}", name, flowsta_hc_lines::TARGET_TRIPLE)).exists();
        if ![KIT_06.conductor.sidecar, KIT_06.lair.sidecar, KIT_07.conductor.sidecar, KIT_07.lair.sidecar].iter().all(|n| have(n)) {
            eprintln!("the Holochain sidecars are not in src-tauri/binaries: skipped");
            return;
        }
        let root = std::env::temp_dir().join(format!("yoai-kits-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let resources = Path::new(env!("CARGO_MANIFEST_DIR")).join("resources");
        let cache = KitCache::new(kits_dir(&root));
        let sidecars = flowsta_hc_lines::SidecarSource { dev_dir: Some(binaries), exe_dir: None, kit_dirs: Vec::new(), allow_path_lookup: false };
        for kit in [&KIT_06, &KIT_07] {
            let first = cache.put(kit, &sidecars, &resources).expect("cached");
            assert_eq!(first.written.len(), 2 + kit.files.len(), "the conductor, the key store and each file");
            let again = cache.put(kit, &sidecars, &resources).expect("cached again");
            assert!(again.written.is_empty(), "nothing changed, nothing written: {:?}", again.written);
            let m = cache.manifest(kit.line).expect("a manifest");
            assert_eq!(m.entries.len(), 2 + kit.files.len());
        }
        let _ = std::fs::remove_dir_all(&root);
    }
}
