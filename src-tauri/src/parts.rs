//! The parts a release needs beside the app itself: the NVIDIA engine and
//! the Projects helper. A release's signed manifest names them (`parts`:
//! a name, a target, a version, a size and a SHA-256, never an address), so
//! the version doing the updating can fetch the ones this person has before
//! it offers the restart, and the new version starts complete.
//!
//! `parts.json` beside this crate is what the release build publishes for
//! the release tool to put in the manifest; a test here holds it to the
//! pins the app is compiled with.

use flowsta_update_check::Part;
use tauri::AppHandle;

/// Fetch, ahead of the update, each named part this install has and the
/// new version changes. Each is checked against the manifest's hash. A
/// failure is logged and left to the new version's first start. Returns
/// how many were fetched.
pub(crate) async fn fetch_ahead(app: &AppHandle, parts: &[Part]) -> usize {
    let mut fetched = 0;
    for part in parts {
        let outcome = if part.name == crate::engine::PART_NAME && crate::engine::wants_ahead(app, &part.target, &part.version) {
            Some(crate::engine::fetch_ahead(app, &part.version, &part.sha256).await)
        } else if part.name == crate::build_install::PART_NAME && crate::build_install::wants_ahead(app, &part.target, &part.version) {
            Some(crate::build_install::fetch_ahead(app, &part.version, &part.sha256).await)
        } else {
            None
        };
        match outcome {
            Some(Ok(())) => fetched += 1,
            Some(Err(e)) => log::warn!("[update] {} {} was not fetched ahead (the new version fetches it at its first start): {}", part.name, part.version, e),
            None => {}
        }
    }
    fetched
}

#[cfg(test)]
mod tests {
    #[derive(serde::Deserialize, Debug, PartialEq, Eq, PartialOrd, Ord)]
    struct Listed {
        name: String,
        target: String,
        version: String,
        size: u64,
        sha256: String,
    }

    #[test]
    fn parts_json_names_exactly_what_this_app_pins() {
        let listed: Vec<Listed> = serde_json::from_str(include_str!("../parts.json")).expect("parts.json is a list of parts");
        assert!(listed.iter().all(|p| p.size > 0 && p.sha256.len() == 64), "every part has a size and a SHA-256");
        let mut sorted: Vec<&Listed> = listed.iter().collect();
        sorted.sort();
        sorted.dedup_by(|a, b| a.name == b.name && a.target == b.target);
        assert_eq!(sorted.len(), listed.len(), "no part is named twice for a target");

        // The NVIDIA engine: every platform with a build, at the pinned tag,
        // size and hash.
        let engine: Vec<(String, String, u64, String)> = {
            let mut v: Vec<_> = listed.iter().filter(|p| p.name == crate::engine::PART_NAME).map(|p| (p.target.clone(), p.version.clone(), p.size, p.sha256.clone())).collect();
            v.sort();
            v
        };
        let mut pinned_engine = crate::engine::pinned_parts();
        pinned_engine.sort();
        assert_eq!(engine, pinned_engine, "parts.json and engine.rs disagree: move them together");

        // The Projects helper: every platform with a build, at the pinned
        // version and hash (its size is carried by parts.json alone).
        let helper: Vec<(String, String, String)> = {
            let mut v: Vec<_> = listed.iter().filter(|p| p.name == crate::build_install::PART_NAME).map(|p| (p.target.clone(), p.version.clone(), p.sha256.clone())).collect();
            v.sort();
            v
        };
        let mut pinned_helper = crate::build_install::pinned_parts();
        pinned_helper.sort();
        assert_eq!(helper, pinned_helper, "parts.json and build_install.rs disagree: move them together");

        assert_eq!(engine.len() + helper.len(), listed.len(), "parts.json names a part this app does not know");
    }
}
