//! The transcript DNA v2 on a real Holochain 0.7.1 conductor.
//!
//! Build first, with build.sh's RUSTFLAGS: `./build.sh` in `v2`, then
//! `RUSTFLAGS='--cfg getrandom_backend="custom"' cargo build --release
//! --target wasm32-unknown-unknown` in `tests/hostile` (its own target
//! folder). Then `cargo test` here.
//!
//! Two cells on one DNA in one conductor stand for two devices of one
//! person (the founding key is the first device's agent); a third cell runs
//! a stranger's coordinator (`tests/hostile`) beside the real one: same
//! DNA hash, same network.

use holochain::prelude::*;
use holochain::sweettest::*;
use std::path::Path;
use transcript_coordinator::{
    ConversationKeys, ConversationsPageInput, DeleteConversationInput, EntriesPageInput, KeyItem, MemoryRowsInput, MyEntriesPage, MyEntriesPageInput,
    RecordMessageInput, RecordsPage, SaveMemoryRowInput, StartConversationInput,
};

const ZOME: &str = "transcript";

/// The bundle, with `hostile` added beside its coordinator when asked
/// (coordinators are not part of the DNA hash).
async fn dna(hostile: bool) -> DnaFile {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../workdir/yourown_ai_transcript_v2.dna");
    let mut dna = SweetDnaFile::from_bundle(&path).await.expect("v2/workdir/yourown_ai_transcript_v2.dna loads (run build.sh first)");
    if hostile {
        let file = Path::new(env!("CARGO_MANIFEST_DIR")).join("hostile/target/wasm32-unknown-unknown/release/transcript_hostile_test.wasm");
        let wasm = DnaWasm::from(std::fs::read(&file).expect("transcript_hostile_test.wasm is built"));
        let hash = holo_hash::WasmHash::with_data(&wasm).await;
        let def = ZomeDef::Wasm(WasmZomeDef::new(hash, Some(vec!["transcript_integrity".into()])));
        dna.update_coordinators(vec![(ZomeName::from("hostile"), def.into())], vec![wasm]).await.unwrap();
    }
    dna
}

/// A conversation's anchor as the app derives it (here: any 32 bytes).
fn anchor(n: u8) -> ExternalHash {
    ExternalHash::from_raw_32(vec![n; 32])
}
/// A message's key as the app derives it.
fn key(n: u8) -> Vec<u8> {
    vec![n; 32]
}
fn start(founding: &AgentPubKey, conv: &ExternalHash) -> StartConversationInput {
    StartConversationInput { cipher: vec![7; 48], nonce: vec![1; 24], anchor: founding.clone(), conversation_anchor: conv.clone() }
}
fn message(conv: &ExternalHash, k: u8) -> RecordMessageInput {
    RecordMessageInput { conversation_anchor: conv.clone(), message_key: key(k), cipher: vec![8; 64], nonce: vec![2; 24] }
}
fn conversations(founding: &AgentPubKey) -> ConversationsPageInput {
    ConversationsPageInput { anchor: founding.clone(), before: None, limit: 10 }
}
fn messages(conv: &ExternalHash) -> EntriesPageInput {
    EntriesPageInput { conversation_anchor: conv.clone(), after: None, limit: 10 }
}

async fn refused<I: serde::Serialize + std::fmt::Debug>(conductor: &SweetConductor, zome: &SweetZome, f: &str, input: I, expect: &str) {
    let r: Result<serde::de::IgnoredAny, _> = conductor.call_fallible(zome, f, input).await;
    let e = format!("{:?}", r.expect_err(f));
    assert!(e.contains(expect), "{f}: expected \"{expect}\", got {e}");
}

#[tokio::test(flavor = "multi_thread")]
async fn two_devices_share_a_conversation_by_its_anchor() {
    let dna = dna(false).await;
    let mut conductor = SweetConductor::standard().await;
    let a = conductor.setup_app("device-a", [&dna]).await.unwrap().into_cells().remove(0);
    let b = conductor.setup_app("device-b", [&dna]).await.unwrap().into_cells().remove(0);
    let (az, bz) = (a.zome(ZOME), b.zome(ZOME));
    let founding = a.agent_pubkey().clone();
    let conv = anchor(1);

    let _: ActionHash = conductor.call(&az, "start_conversation", start(&founding, &conv)).await;
    let _: ActionHash = conductor.call(&az, "record_message", message(&conv, 1)).await;
    // Device B adds to device A's conversation by its anchor.
    let _: ActionHash = conductor.call(&bz, "record_message", message(&conv, 2)).await;
    await_consistency([&a, &b]).await.expect("both devices see both records");
    for z in [&az, &bz] {
        let page: RecordsPage = conductor.call(z, "get_conversations_page", conversations(&founding)).await;
        assert_eq!((page.total, page.items.len()), (1, 1));
        assert_eq!(page.items[0].key, conv.get_raw_39().to_vec(), "the list link carries the anchor");
        let page: RecordsPage = conductor.call(z, "get_conversation_entries_page", messages(&conv)).await;
        assert_eq!((page.total, page.items.len()), (2, 2), "one message from each device");
    }
    let mine: MyEntriesPage = conductor.call(&az, "get_my_entries_page", MyEntriesPageInput { anchor: founding.clone(), after_seq: None, limit: 10 }).await;
    assert_eq!(mine.total, 1, "device A's own chain holds its one message");
    assert_eq!(mine.items[0].conversation_anchor, conv);

    // Both devices copied the same conversation and message (a move on two
    // devices at once): still one of each.
    let _: ActionHash = conductor.call(&bz, "start_conversation", start(&founding, &conv)).await;
    let _: ActionHash = conductor.call(&bz, "record_message", message(&conv, 1)).await;
    await_consistency([&a, &b]).await.expect("the copies arrive");
    for z in [&az, &bz] {
        let page: RecordsPage = conductor.call(z, "get_conversations_page", conversations(&founding)).await;
        assert_eq!(page.total, 1, "two copies of one conversation read as one");
        let page: RecordsPage = conductor.call(z, "get_conversation_entries_page", messages(&conv)).await;
        assert_eq!((page.total, page.items.len()), (2, 2), "two copies of one message read as one");
        let keys: ConversationKeys = conductor.call(z, "get_conversation_keys", founding.clone()).await;
        assert_eq!(keys.conversations.len(), 1);
        let msg_keys: Vec<KeyItem> = conductor.call(z, "get_message_keys", conv.clone()).await;
        assert_eq!(msg_keys.iter().map(|k| k.key.clone()).collect::<Vec<_>>(), vec![key(1), key(2)]);
    }

    // Memory rows live beside conversations and are read apart from them.
    let _: ActionHash = conductor
        .call(&az, "save_memory_row", SaveMemoryRowInput { cipher: vec![5; 32], nonce: vec![3; 24], anchor: founding.clone().into() })
        .await;
    let rows: Vec<Record> = conductor.call(&az, "get_memory_rows", MemoryRowsInput { anchor: founding.clone().into() }).await;
    assert_eq!(rows.len(), 1);
    let page: RecordsPage = conductor.call(&az, "get_conversations_page", conversations(&founding)).await;
    assert_eq!(page.total, 1, "a memory row is not a conversation");

    // Device B deletes the conversation: its own records go, a tombstone
    // hides the conversation on both, and device A's records stay A's.
    let deleted: u32 = conductor.call(&bz, "delete_conversation", DeleteConversationInput { anchor: founding.clone(), conversation_anchor: conv.clone() }).await;
    assert_eq!(deleted, 3, "B's copy of the conversation and its two messages");
    await_consistency([&a, &b]).await.expect("the tombstone arrives");
    for z in [&az, &bz] {
        let page: RecordsPage = conductor.call(z, "get_conversations_page", conversations(&founding)).await;
        assert_eq!(page.total, 0, "hidden on both devices");
        let keys: ConversationKeys = conductor.call(z, "get_conversation_keys", founding.clone()).await;
        assert_eq!(keys.tombstoned, vec![conv.get_raw_39().to_vec()]);
    }
    let mine: MyEntriesPage = conductor.call(&az, "get_my_entries_page", MyEntriesPageInput { anchor: founding.clone(), after_seq: None, limit: 10 }).await;
    assert_eq!(mine.total, 0, "a tombstoned conversation's messages are not listed");
}

#[tokio::test(flavor = "multi_thread")]
async fn a_device_alone_reads_its_own_keys_locally() {
    // The move reads the keys from this conductor first and adds the
    // network's answer when it comes (a peer that cannot be reached after a
    // cold start held whole passes before). A device with no peer gets its
    // own conversations and tombstones from the local read, the same as from
    // the network read.
    let dna = dna(false).await;
    let mut conductor = SweetConductor::standard().await;
    let a = conductor.setup_app("device-a", [&dna]).await.unwrap().into_cells().remove(0);
    let az = a.zome(ZOME);
    let founding = a.agent_pubkey().clone();
    let (kept, gone) = (anchor(1), anchor(2));
    let _: ActionHash = conductor.call(&az, "start_conversation", start(&founding, &kept)).await;
    let _: ActionHash = conductor.call(&az, "start_conversation", start(&founding, &gone)).await;
    let _: u32 = conductor.call(&az, "delete_conversation", DeleteConversationInput { anchor: founding.clone(), conversation_anchor: gone.clone() }).await;
    let local: ConversationKeys = conductor.call(&az, "get_conversation_keys_local", founding.clone()).await;
    let network: ConversationKeys = conductor.call(&az, "get_conversation_keys", founding.clone()).await;
    assert!(local.conversations.iter().any(|k| k.key == kept.get_raw_39().to_vec()), "the kept conversation's key, read locally");
    assert_eq!(local.tombstoned, vec![gone.get_raw_39().to_vec()], "the tombstone, read locally");
    assert_eq!(local.conversations, network.conversations);
    assert_eq!(local.tombstoned, network.tombstoned);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_malformed_record_is_refused() {
    let dna = dna(false).await;
    let mut conductor = SweetConductor::standard().await;
    let a = conductor.setup_app("device-a", [&dna]).await.unwrap().into_cells().remove(0);
    let az = a.zome(ZOME);
    let founding = a.agent_pubkey().clone();
    let conv = anchor(1);
    refused(&conductor, &az, "start_conversation", StartConversationInput { nonce: vec![1; 23], ..start(&founding, &conv) }, "Nonce must be 24 bytes").await;
    refused(&conductor, &az, "start_conversation", StartConversationInput { cipher: vec![], ..start(&founding, &conv) }, "Cipher cannot be empty").await;
    refused(&conductor, &az, "start_conversation", StartConversationInput { cipher: vec![7; 1_048_577], ..start(&founding, &conv) }, "exceeds maximum size").await;
    refused(&conductor, &az, "record_message", RecordMessageInput { message_key: vec![1; 31], ..message(&conv, 1) }, "A message key is 32 bytes").await;
}

#[derive(serde::Serialize, Debug)]
struct LinkInput {
    base: AnyLinkableHash,
    target: AnyLinkableHash,
    tag: Vec<u8>,
}
#[derive(serde::Serialize, Debug)]
struct RemoveInput {
    base: AnyLinkableHash,
    kind: String,
}

#[tokio::test(flavor = "multi_thread")]
async fn a_strangers_writes_are_refused() {
    let (ours, hostile) = (dna(false).await, dna(true).await);
    assert_eq!(ours.dna_hash(), hostile.dna_hash());
    let mut conductor = SweetConductor::standard().await;
    let a = conductor.setup_app("device-a", [&ours]).await.unwrap().into_cells().remove(0);
    let m = conductor.setup_app("mallory", [&hostile]).await.unwrap().into_cells().remove(0);
    let (az, mh) = (a.zome(ZOME), m.zome("hostile"));
    let founding = a.agent_pubkey().clone();
    let conv = anchor(1);
    let theirs: ActionHash = conductor.call(&az, "start_conversation", start(&founding, &conv)).await;
    let their_message: ActionHash = conductor.call(&az, "record_message", message(&conv, 1)).await;
    let own: ActionHash = conductor.call(&mh, "own_entry", ()).await;
    await_consistency([&a, &m]).await.expect("both see each other's records");
    let mut conv_tag = vec![b'c'];
    conv_tag.extend_from_slice(conv.get_raw_39());
    let mut msg_tag = vec![b'm'];
    msg_tag.extend(key(9));

    // Deletes and updates.
    refused(&conductor, &mh, "delete_record", theirs.clone(), "Only a record's author can delete it").await;
    refused(&conductor, &mh, "delete_record", their_message.clone(), "Only a record's author can delete it").await;
    refused(&conductor, &mh, "update_record", theirs.clone(), "never updated").await;
    refused(&conductor, &mh, "update_record", own.clone(), "never updated").await;
    // Misfiled or malformed links.
    let link = |base: AnyLinkableHash, target: AnyLinkableHash, tag: Vec<u8>| LinkInput { base, target, tag };
    refused(&conductor, &mh, "list_link", link(founding.clone().into(), theirs.clone().into(), conv_tag.clone()), "filed by its own author").await;
    refused(&conductor, &mh, "list_link", link(founding.clone().into(), own.clone().into(), vec![]), "c + an anchor, mem or tomb").await;
    refused(&conductor, &mh, "list_link", link(conv.clone().into(), own.clone().into(), conv_tag.clone()), "listed under the AI's founding key").await;
    refused(&conductor, &mh, "list_link", link(founding.clone().into(), own.clone().into(), b"tomb".to_vec()), "names a conversation's anchor").await;
    refused(&conductor, &mh, "message_link", link(theirs.clone().into(), own.clone().into(), msg_tag.clone()), "filed under its conversation's anchor").await;
    refused(&conductor, &mh, "message_link", link(conv.clone().into(), own.clone().into(), b"x".to_vec()), "m + the keyed hash").await;
    refused(&conductor, &mh, "message_link", link(conv.clone().into(), their_message.clone().into(), msg_tag.clone()), "filed by its own author").await;
    // Link removals.
    refused(&conductor, &mh, "remove_links", RemoveInput { base: founding.clone().into(), kind: "list".into() }, "Only a link's author can remove it").await;
    refused(&conductor, &mh, "remove_links", RemoveInput { base: conv.clone().into(), kind: "message".into() }, "Only a link's author can remove it").await;

    // Device A reads everything as it wrote it.
    let page: RecordsPage = conductor.call(&az, "get_conversations_page", conversations(&founding)).await;
    assert_eq!(page.total, 1);
    let page: RecordsPage = conductor.call(&az, "get_conversation_entries_page", messages(&conv)).await;
    assert_eq!(page.total, 1);

    // The one write any agent on the network may make: a tombstone (the
    // network seed is per person, so only the person's devices are on it).
    let _: ActionHash = conductor.call(&mh, "list_link", link(founding.clone().into(), conv.clone().into(), b"tomb".to_vec())).await;
}
