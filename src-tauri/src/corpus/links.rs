//! A document stays linked to its file.
//!
//! This module holds the parts of that with no database in them, so they can
//! be tested on their own:
//!  - what state a file is in right now: present, ONLINE ONLY (a cloud
//!    placeholder whose data lives in OneDrive / iCloud / Dropbox - reading it
//!    would download it), or missing;
//!  - the file's IDENTITY: the BLAKE3 hash of its bytes (the same hash an
//!    Iroh blob of the file would have, so a Vault file index can be asked
//!    for it later - works without any Vault today);
//!  - an ANCHOR: "Documents > Research/paper.pdf" instead of an absolute path,
//!    so a link survives a different user name, drive letter or computer.
//!
//! Under OneDrive's folder backup (Windows) or iCloud's Desktop & Documents
//! (macOS) a person's own Documents folder IS a cloud folder. Nothing here
//! ever opens a file without checking first, and nothing memory-maps.

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// What a linked file looks like on disk right now.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FileState {
    /// On this computer and readable: size in bytes, modified time in seconds.
    Present { size: i64, mtime: i64 },
    /// The name is here, the data is in a cloud. Reading would download it.
    OnlineOnly,
    Missing,
}

/// Windows: the file's data is not (all) local - FILE_ATTRIBUTE_OFFLINE
/// (0x1000), RECALL_ON_OPEN (0x40000), RECALL_ON_DATA_ACCESS (0x400000).
/// PINNED / UNPINNED are the person's wish, not the file's state: ignored.
pub const WINDOWS_NOT_LOCAL: u32 = 0x0000_1000 | 0x0004_0000 | 0x0040_0000;
/// macOS: `SF_DATALESS` in `st_flags` - a dataless (evicted) file or folder.
pub const MACOS_DATALESS: u32 = 0x4000_0000;

/// An evicted iCloud file on macOS 13 and earlier is replaced by a tiny
/// hidden stub named `.<name>.icloud`.
pub fn is_icloud_stub(name: &str) -> bool {
    name.len() > 8 && name.starts_with('.') && name.ends_with(".icloud")
}

/// Is this entry a cloud placeholder? Reads attributes only - never data.
/// On Linux there is no general marker: always false (the UI never claims
/// otherwise).
pub fn is_online_only(meta: &std::fs::Metadata, name: &str) -> bool {
    if is_icloud_stub(name) {
        return true;
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        return meta.file_attributes() & WINDOWS_NOT_LOCAL != 0;
    }
    #[cfg(target_os = "macos")]
    {
        use std::os::macos::fs::MetadataExt;
        return meta.st_flags() & MACOS_DATALESS != 0;
    }
    #[allow(unreachable_code)]
    {
        let _ = meta;
        false
    }
}

/// The state of one path. `symlink_metadata`: attributes only, and it does
/// not follow a link into somewhere else.
pub fn file_state(path: &Path) -> FileState {
    let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
    let Ok(meta) = std::fs::symlink_metadata(path) else {
        // The real name may be gone because iCloud left a stub in its place.
        if let (Some(dir), false) = (path.parent(), name.is_empty()) {
            if dir.join(format!(".{name}.icloud")).exists() {
                return FileState::OnlineOnly;
            }
        }
        return FileState::Missing;
    };
    if is_online_only(&meta, name) {
        return FileState::OnlineOnly;
    }
    if !meta.is_file() {
        return FileState::Missing;
    }
    let mtime = meta
        .modified()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    FileState::Present { size: meta.len() as i64, mtime }
}

/// On macOS a read of a dataless file on THIS thread fails (EDEADLK) instead
/// of downloading it - the backstop under the checks above. A no-op
/// elsewhere. Call once on a thread that walks or hashes.
pub fn never_download_on_this_thread() {
    #[cfg(target_os = "macos")]
    {
        extern "C" {
            fn setiopolicy_np(iotype: i32, scope: i32, policy: i32) -> i32;
        }
        const IOPOL_TYPE_VFS_MATERIALIZE_DATALESS_FILES: i32 = 3;
        const IOPOL_SCOPE_THREAD: i32 = 1;
        const IOPOL_MATERIALIZE_DATALESS_FILES_OFF: i32 = 1;
        // Best effort: an older system that refuses leaves the checks above.
        unsafe {
            let _ = setiopolicy_np(
                IOPOL_TYPE_VFS_MATERIALIZE_DATALESS_FILES,
                IOPOL_SCOPE_THREAD,
                IOPOL_MATERIALIZE_DATALESS_FILES_OFF,
            );
        }
    }
}

/// The file's identity: BLAKE3 of its bytes, hex. Streams (a library
/// document has no size limit) and never maps. `None` when the file is not
/// fully on this computer - checked immediately before the open.
pub fn content_id(path: &Path) -> Result<Option<String>, String> {
    if !matches!(file_state(path), FileState::Present { .. }) {
        return Ok(None);
    }
    never_download_on_this_thread();
    let mut f = std::fs::File::open(path).map_err(|e| e.to_string())?;
    let mut hasher = blake3::Hasher::new();
    hasher.update_reader(&mut f).map_err(|e| e.to_string())?;
    Ok(Some(hasher.finalize().to_hex().to_string()))
}

/// The same identity for bytes already in memory (tests, small inputs).
pub fn content_id_of(bytes: &[u8]) -> String {
    blake3::hash(bytes).to_hex().to_string()
}

/// A place that means the same thing on every computer.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct Anchor {
    /// "documents" | "downloads" | "desktop" | "pictures" | "videos" |
    /// "audio" | "home"
    pub base: String,
    /// Path inside it, `/`-separated whatever the OS.
    pub rel: String,
}

/// The anchor for `path`, given this machine's well-known folders as
/// (base, folder) pairs. The most specific folder wins, so a file in
/// Documents is "documents", not "home". Pure: the folders are passed in.
pub fn anchor_among(path: &Path, bases: &[(&str, PathBuf)]) -> Option<Anchor> {
    let mut best: Option<(usize, Anchor)> = None;
    for (base, dir) in bases {
        let Ok(rel) = path.strip_prefix(dir) else { continue };
        let depth = dir.components().count();
        if best.as_ref().map_or(true, |(d, _)| depth > *d) {
            let rel = rel.components().filter_map(|c| c.as_os_str().to_str()).collect::<Vec<_>>().join("/");
            best = Some((depth, Anchor { base: base.to_string(), rel }));
        }
    }
    best.map(|(_, a)| a)
}

/// Where an anchor points on THIS machine.
pub fn resolve_among(anchor: &Anchor, bases: &[(&str, PathBuf)]) -> Option<PathBuf> {
    let (_, dir) = bases.iter().find(|(b, _)| *b == anchor.base)?;
    let mut p = dir.clone();
    for part in anchor.rel.split('/').filter(|s| !s.is_empty() && *s != ".." && *s != ".") {
        p.push(part);
    }
    Some(p)
}

/// This machine's well-known folders. A folder the OS cannot name (Linux
/// without `user-dirs.dirs`) is simply left out; `home` is always there.
pub fn known_folders(app: &tauri::AppHandle) -> Vec<(&'static str, PathBuf)> {
    use tauri::Manager;
    let p = app.path();
    [
        ("documents", p.document_dir().ok()),
        ("downloads", p.download_dir().ok()),
        ("desktop", p.desktop_dir().ok()),
        ("pictures", p.picture_dir().ok()),
        ("videos", p.video_dir().ok()),
        ("audio", p.audio_dir().ok()),
        ("home", p.home_dir().ok()),
    ]
    .into_iter()
    .filter_map(|(b, d)| d.map(|d| (b, d)))
    .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_identity_is_plain_blake3_of_the_bytes_streamed_or_not() {
        // The published BLAKE3 test vector for the empty input.
        assert_eq!(content_id_of(b""), "af1349b9f5f9a1a6a0404dea36dcc9499bcb25c9adc112b7cc9a93cae41f3262");
        let dir = std::env::temp_dir().join(format!("yoai-links-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let f = dir.join("a note.md");
        let body = "- tidal range 4.2 meters\n".repeat(50_000);
        std::fs::write(&f, &body).unwrap();
        assert_eq!(content_id(&f).unwrap().as_deref(), Some(content_id_of(body.as_bytes()).as_str()));
        assert!(matches!(file_state(&f), FileState::Present { size, .. } if size == body.len() as i64));
        std::fs::remove_file(&f).unwrap();
        assert_eq!(file_state(&f), FileState::Missing);
        assert_eq!(content_id(&f).unwrap(), None, "a file that is not here has no identity to read");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn an_evicted_icloud_file_is_online_only_not_missing() {
        assert!(is_icloud_stub(".Report.pdf.icloud"));
        assert!(!is_icloud_stub("Report.pdf"));
        assert!(!is_icloud_stub(".icloud"));
        let dir = std::env::temp_dir().join(format!("yoai-stub-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join(".Report.pdf.icloud"), b"stub").unwrap();
        assert_eq!(file_state(&dir.join("Report.pdf")), FileState::OnlineOnly);
        assert_eq!(file_state(&dir.join("Other.pdf")), FileState::Missing);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_placeholder_bits_are_the_documented_ones() {
        assert_eq!(WINDOWS_NOT_LOCAL, 0x0044_1000);
        assert_eq!(MACOS_DATALESS, 0x4000_0000);
    }

    #[test]
    fn an_anchor_names_the_nearest_known_folder_and_resolves_on_another_machine() {
        let here = [("home", PathBuf::from("/home/eric")), ("documents", PathBuf::from("/home/eric/Documents"))];
        let a = anchor_among(Path::new("/home/eric/Documents/Research/paper.pdf"), &here).unwrap();
        assert_eq!(a, Anchor { base: "documents".into(), rel: "Research/paper.pdf".into() });
        assert_eq!(anchor_among(Path::new("/home/eric/notes.md"), &here).unwrap().base, "home");
        assert_eq!(anchor_among(Path::new("/mnt/usb/paper.pdf"), &here), None);

        let there = [("documents", PathBuf::from("/Users/eric/OneDrive/Documents"))];
        assert_eq!(resolve_among(&a, &there).unwrap(), PathBuf::from("/Users/eric/OneDrive/Documents/Research/paper.pdf"));
        assert_eq!(resolve_among(&Anchor { base: "videos".into(), rel: "x".into() }, &there), None);
        // A relative path never climbs out of its folder.
        let sneaky = Anchor { base: "documents".into(), rel: "../../etc/passwd".into() };
        assert_eq!(resolve_among(&sneaky, &there).unwrap(), PathBuf::from("/Users/eric/OneDrive/Documents/etc/passwd"));
    }
}
