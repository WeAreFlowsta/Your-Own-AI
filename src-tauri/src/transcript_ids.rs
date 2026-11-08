//! Record ids on the transcript networks, and the addresses every device
//! derives from them.
//!
//! **A record's id is its v1 action hash whenever it has one**: every record
//! that exists today, and every record written while the app still writes
//! to the 0.6 line (written there first, so it takes the hash v1 gives it).
//! The app already keys its caches, search index, backup labels and the
//! frontend by that hash, so none of them changes. Only a record written to
//! the 0.7 line alone gets a fresh id. Ids are opaque strings.
//!
//! On the 0.7 line (DNA v2) records are found by keyed hashes of their ids,
//! under the person's data key, so peers and the zome learn nothing:
//! - a conversation's anchor, where its messages hang and what its list
//!   link and tombstone name;
//! - each record's key, carried in its link's tag, which lets a reader
//!   collapse copies and a move skip what is already there without
//!   decrypting.

use sha2::{Digest, Sha256};

/// A v1 record's id: its 39-byte action hash, hex (what the app has always
/// keyed by).
pub fn id_of_v1(raw_39: &[u8]) -> String {
    hex::encode(raw_39)
}

/// The first bytes of a fresh id's core: how this app tells a fresh id from
/// a v1 action hash of the same shape.
const FRESH_TAG: [u8; 4] = *b"yoa7";

/// A fresh id, for a record written to the 0.7 line alone. It has the shape
/// of a v1 action hash (39 bytes, hex) with a tag in its core, so an older
/// release given one (a computer taken back to it by hand) looks it up and
/// finds nothing, where a shorter id stopped it outright.
pub fn fresh_id() -> String {
    let mut core = [0u8; 32];
    core[..4].copy_from_slice(&FRESH_TAG);
    rand::RngCore::fill_bytes(&mut rand::rngs::OsRng, &mut core[4..]);
    hex::encode(holochain_types::prelude::ActionHash::from_raw_32(core.to_vec()).get_raw_39())
}

/// The v1 action hash an id is, when it is one: never for a fresh id, and
/// never a panic on an id of another shape.
pub fn v1_hash(id: &str) -> Option<holochain_types::prelude::ActionHash> {
    let raw = hex::decode(id).ok().filter(|b| b.len() == 39)?;
    if raw[3..7] == FRESH_TAG {
        return None;
    }
    holochain_types::prelude::ActionHash::try_from_raw_39(raw).ok()
}

/// The keyed hash both addresses use: the memory scopes' scheme
/// (`memory_rows::scope_anchor`), over `<what>:<id>`.
fn keyed(data_key: &[u8; 32], what: &str, id: &str) -> [u8; 32] {
    let mut h = Sha256::new();
    h.update(b"yoai-memory-anchor-v1\0");
    h.update(data_key);
    h.update(what.as_bytes());
    h.update(b":");
    h.update(id.as_bytes());
    h.finalize().into()
}

/// The core of a conversation's anchor (an `ExternalHash`), from its id.
pub fn conversation_anchor(data_key: &[u8; 32], conversation_id: &str) -> [u8; 32] {
    keyed(data_key, "conversation", conversation_id)
}

/// A message's (or annotation's) key, carried in its link's tag.
pub fn message_key(data_key: &[u8; 32], message_id: &str) -> [u8; 32] {
    keyed(data_key, "message", message_id)
}

/// A conversation's plaintext as v2 holds it: v1's, with its id.
pub fn conversation_plaintext_v2(mut plain: serde_json::Value, id: &str) -> serde_json::Value {
    if let Some(o) = plain.as_object_mut() {
        o.insert("id".into(), id.into());
    }
    plain
}

/// A message's (or annotation's) plaintext as v2 holds it: v1's, with its
/// id and its conversation's.
pub fn message_plaintext_v2(mut plain: serde_json::Value, id: &str, conversation_id: &str) -> serde_json::Value {
    if let Some(o) = plain.as_object_mut() {
        o.insert("id".into(), id.into());
        o.insert("conversation_id".into(), conversation_id.into());
    }
    plain
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_fresh_id_has_a_v1_hashs_shape_and_is_never_taken_for_one() {
        let id = fresh_id();
        assert_eq!(id.len(), 78, "39 bytes, hex: an older release reads it without stopping");
        let raw = hex::decode(&id).unwrap();
        assert!(holochain_types::prelude::ActionHash::try_from_raw_39(raw).is_ok(), "a valid action hash shape");
        assert!(super::v1_hash(&id).is_none(), "but never a v1 record");
        assert_ne!(fresh_id(), id);
        // A real v1 hash is one; other shapes are not, and never panic.
        let real = hex::encode(holochain_types::prelude::ActionHash::from_raw_32(vec![9u8; 32]).get_raw_39());
        assert!(super::v1_hash(&real).is_some());
        assert!(super::v1_hash(&"ab".repeat(32)).is_none(), "the older 64-hex shape");
        assert!(super::v1_hash("not hex").is_none());
        assert!(super::v1_hash("").is_none());
    }

    /// A v1 action hash as the app has it: the 3-byte prefix, 32 bytes, the
    /// 4 location bytes.
    fn v1_hash(n: u8) -> Vec<u8> {
        let mut h = vec![0x84, 0x29, 0x24];
        h.extend([n; 32]);
        h.extend([1, 2, 3, 4]);
        h
    }

    #[test]
    fn a_v1_records_id_is_the_hash_the_app_already_keys_by() {
        let h = v1_hash(7);
        let id = id_of_v1(&h);
        assert_eq!(id.len(), 78);
        assert_eq!(hex::decode(&id).unwrap(), h, "the id decodes back to the action hash");
        assert!(id.starts_with("842924"), "the spelling the caches and labels already use");
    }

    #[test]
    fn fresh_ids_are_random_and_never_mistaken_for_a_v1_hash() {
        let (a, b) = (fresh_id(), fresh_id());
        assert_ne!(a, b);
        assert!(super::v1_hash(&a).is_none(), "the tag tells a fresh id from a v1 hash of the same shape");
        assert!(super::v1_hash(&id_of_v1(&v1_hash(1))).is_some());
    }

    #[test]
    fn every_device_derives_the_same_addresses_and_each_kind_its_own() {
        let key = [5u8; 32];
        let id = id_of_v1(&v1_hash(9));
        assert_eq!(conversation_anchor(&key, &id), conversation_anchor(&key, &id), "the same on every device");
        assert_ne!(conversation_anchor(&key, &id), conversation_anchor(&[6u8; 32], &id), "another person, another anchor");
        assert_ne!(conversation_anchor(&key, &id), message_key(&key, &id), "a conversation and a message never share an address");
        assert_ne!(conversation_anchor(&key, &id), conversation_anchor(&key, &id_of_v1(&v1_hash(8))));
    }

    #[test]
    fn v2_plaintext_is_v1_with_its_ids() {
        let conv = serde_json::json!({"title": "Plans", "started_at": 5, "ai_personality_id": "ab"});
        let v2 = conversation_plaintext_v2(conv.clone(), "c1");
        assert_eq!(v2["id"], "c1");
        assert_eq!(v2["title"], conv["title"], "nothing else changes");
        let msg = serde_json::json!({"role": "user", "content": "hi", "sequence": 3, "timestamp": 9});
        let m2 = message_plaintext_v2(msg, "m1", "c1");
        assert_eq!((m2["id"].as_str(), m2["conversation_id"].as_str(), m2["sequence"].as_u64()), (Some("m1"), Some("c1"), Some(3)));
    }
}
