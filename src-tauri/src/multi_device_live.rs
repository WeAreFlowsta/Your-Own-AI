//! Two conductors, one person's network - the behaviours the multi-device
//! plan cannot prove by reading (build-docs `MULTI_DEVICE.md` §9): a
//! conversation started by one device's agent is listed by the other at the
//! first agent's key (the founding key as an address), a message appended
//! from the second device lands under a conversation the first authored,
//! and two conductors on one seed find each other through the rendezvous.
//!
//! Starts two REAL conductors from the bundled binary with in-process key
//! stores on the staging rendezvous (the app's own config points bootstrap,
//! signal and relay at a localhost black hole - one conductor never needed
//! peers). Run with the app closed:
//! `cargo test --lib -- --ignored live_md --nocapture`

#[cfg(test)]
mod tests {
    use holochain_client::{AdminWebsocket, AppWebsocket, InstallAppPayload, ZomeCallTarget};
    use holochain_types::prelude::{ActionHash, AgentPubKey, AppBundleSource, ExternIO, FunctionName, RoleName, Timestamp, ZomeName};
    use std::io::Write;
    use std::path::PathBuf;

    fn manifest_dir() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
    }

    /// The staging rendezvous auth material the Vault's test instances use
    /// (`flowsta-vault/scripts/run-test-instance.sh`); `FLOWSTA_AUTH_MATERIAL`
    /// in the environment wins.
    fn auth_material() -> String {
        if let Ok(a) = std::env::var("FLOWSTA_AUTH_MATERIAL") {
            return a;
        }
        std::fs::read_to_string(manifest_dir().join("../../flowsta-vault/scripts/run-test-instance.sh"))
            .ok()
            .and_then(|t| {
                t.split("FLOWSTA_AUTH_MATERIAL=")
                    .nth(1)
                    .map(|r| r.split_whitespace().next().unwrap_or("").to_string())
            })
            .unwrap_or_default()
    }

    struct Device {
        name: String,
        agent: AgentPubKey,
        app: AppWebsocket,
        child: std::process::Child,
        dir: PathBuf,
    }

    impl Drop for Device {
        fn drop(&mut self) {
            let _ = self.child.kill();
            let _ = self.child.wait();
            let _ = std::fs::remove_dir_all(&self.dir);
        }
    }

    impl Device {
        async fn call<I: serde::Serialize + std::fmt::Debug, O: serde::de::DeserializeOwned + std::fmt::Debug>(&self, f: &str, input: I) -> Result<O, String> {
            let out = self
                .app
                .call_zome(
                    ZomeCallTarget::RoleName(RoleName::from(crate::dna::ROLE_NAME)),
                    ZomeName::from("transcript"),
                    FunctionName::from(f),
                    ExternIO::encode(input).map_err(|e| e.to_string())?,
                )
                .await
                .map_err(|e| format!("{}: {}", self.name, e))?;
            out.decode().map_err(|e| e.to_string())
        }
    }

    /// Start a conductor the way the app does (minus the black-hole network),
    /// install the transcript hApp on `seed` under a fresh agent, hot-swap to
    /// the current coordinator, open an app connection.
    async fn device(name: &str, admin_port: u16, seed: &str) -> Device {
        let binary = manifest_dir().join("binaries").join("yourowai-holochain-x86_64-unknown-linux-gnu");
        assert!(binary.exists(), "the bundled conductor binary is needed for this test");
        let resources = manifest_dir().join("resources");
        // A short path: the key store's socket path has a length limit.
        let dir = PathBuf::from(format!("/tmp/yoai-md-{}-{}", std::process::id(), name));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("data")).unwrap();
        std::fs::create_dir_all(dir.join("ks")).unwrap();
        let config_path = dir.join("conductor-config.yaml");
        std::fs::write(
            &config_path,
            format!(
                "data_root_path: '{d}/data'\nkeystore:\n  type: lair_server_in_proc\n  lair_root: '{d}/ks'\nadmin_interfaces:\n- driver:\n    type: websocket\n    port: {p}\n    allowed_origins: '*'\nnetwork:\n  bootstrap_url: https://bootstrap-staging.flowsta.com\n  signal_url: wss://bootstrap-staging.flowsta.com\n  relay_url: https://bootstrap-staging.flowsta.com./\n  base64_auth_material_bootstrap: \"{a}\"\n  base64_auth_material_relay: \"{a}\"\n  request_timeout_s: 240\ndb_sync_strategy: Resilient\n",
                d = dir.display(),
                p = admin_port,
                a = auth_material(),
            ),
        )
        .unwrap();
        let mut child = std::process::Command::new(&binary)
            .arg("--piped")
            .arg("-c")
            .arg(&config_path)
            .stdin(std::process::Stdio::piped())
            .stdout(std::fs::File::create(dir.join("holochain.log")).unwrap())
            .stderr(std::fs::File::create(dir.join("holochain-stderr.log")).unwrap())
            .spawn()
            .expect("start the conductor");
        child.stdin.take().unwrap().write_all(b"live-test\n").unwrap();

        let mut admin = None;
        for _ in 0..60 {
            if let Ok(ws) = AdminWebsocket::connect(format!("localhost:{}", admin_port), Some("your-own-ai".to_string())).await {
                admin = Some(ws);
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(500)).await;
        }
        let admin = admin.expect("admin interface");
        let agent = admin.generate_agent_pub_key().await.expect("agent key");
        let app_id = crate::dna::make_app_id(name);
        admin
            .install_app(InstallAppPayload {
                source: AppBundleSource::Path(resources.join("yourown_ai_transcript_v1_happ.happ")),
                agent_key: Some(agent.clone()),
                installed_app_id: Some(app_id.clone()),
                network_seed: Some(seed.to_string()),
                roles_settings: None,
                ignore_genesis_failure: false,
            })
            .await
            .expect("install the transcript hApp");
        admin.enable_app(app_id.clone()).await.expect("enable");
        crate::dna::update_coordinators_sweep(admin_port, &resources, &dir)
            .await
            .expect("hot-swap to the current coordinator");
        let app_port = admin
            .attach_app_interface(0, None, holochain_client::AllowedOrigins::Any, None)
            .await
            .expect("app interface");
        let app = crate::dna::connect_app_websocket(admin_port, app_port, &app_id)
            .await
            .expect("app connection");
        println!("  {} up: agent {} admin {} app {}", name, agent, admin_port, app_port);
        Device { name: name.to_string(), agent, app, child, dir }
    }

    async fn eventually<F, Fut>(what: &str, seconds: u64, mut check: F)
    where
        F: FnMut() -> Fut,
        Fut: std::future::Future<Output = bool>,
    {
        let started = std::time::Instant::now();
        loop {
            if check().await {
                println!("  {} after {}s", what, started.elapsed().as_secs());
                return;
            }
            assert!(started.elapsed().as_secs() < seconds, "not within {}s: {}", seconds, what);
            tokio::time::sleep(std::time::Duration::from_secs(3)).await;
        }
    }

    #[derive(serde::Serialize, Debug)]
    struct EncryptedInput {
        cipher: Vec<u8>,
        nonce: Vec<u8>,
    }
    #[derive(serde::Serialize, Debug)]
    struct RecordMessageInput {
        conversation_hash: ActionHash,
        cipher: Vec<u8>,
        nonce: Vec<u8>,
    }
    #[derive(serde::Serialize, Debug)]
    struct ConversationsPageInput {
        anchor: Option<AgentPubKey>,
        before: Option<Timestamp>,
        limit: u32,
    }
    #[derive(serde::Serialize, Debug)]
    struct EntriesPageInput {
        conversation_hash: ActionHash,
        after: Option<Timestamp>,
        limit: u32,
    }
    #[derive(serde::Deserialize, Debug)]
    struct Page {
        total: u32,
        missing: u32,
    }
    #[derive(serde::Serialize, Debug)]
    struct DeleteAtInput {
        conversation_hash: ActionHash,
        anchor: Option<AgentPubKey>,
    }
    #[derive(serde::Serialize, Debug)]
    struct StartConversationInput {
        cipher: Vec<u8>,
        nonce: Vec<u8>,
        anchor: Option<AgentPubKey>,
    }

    /// (1) A conversation started on A is listed on B at A's agent key - the
    /// founding key works as an address with no private half involved;
    /// (4) two conductors on one seed find each other through the
    /// rendezvous; and B can append a message under A's conversation, which
    /// A then lists (the link base lives on another device's chain).
    #[tokio::test(flavor = "multi_thread")]
    #[ignore = "starts two real conductors on the staging rendezvous"]
    async fn live_md_a_conversation_on_one_device_lists_on_the_other() {
        let seed = format!("yoai-user-livemd-{:x}", rand::random::<u64>());
        let a = device("a", 47011, &seed).await;
        let b = device("b", 47012, &seed).await;

        let hash: ActionHash = a
            .call("start_conversation", EncryptedInput { cipher: b"ciphertext-a".to_vec(), nonce: vec![7u8; 24] })
            .await
            .expect("A starts a conversation");
        println!("  A started {}", hash);

        let (b_ref, a_key) = (&b, a.agent.clone());
        eventually("B lists A's conversation at A's key", 420, || async {
            let page: Result<Page, String> = b_ref
                .call("get_conversations_page", ConversationsPageInput { anchor: Some(a_key.clone()), before: None, limit: 10 })
                .await;
            match page {
                Ok(p) => {
                    println!("    B sees total {} missing {}", p.total, p.missing);
                    p.total == 1 && p.missing == 0
                }
                Err(e) => {
                    println!("    B not yet: {}", e);
                    false
                }
            }
        })
        .await;

        // B's own list at its own key stays empty: the address is A's key.
        let own: Page = b
            .call("get_conversations_page", ConversationsPageInput { anchor: None, before: None, limit: 10 })
            .await
            .expect("B's own list");
        assert_eq!(own.total, 0, "B's own anchor must not list A's conversation");

        // B appends under A's conversation.
        let appended: Result<ActionHash, String> = b
            .call("record_message", RecordMessageInput { conversation_hash: hash.clone(), cipher: b"from-b".to_vec(), nonce: vec![9u8; 24] })
            .await;
        println!("  B appends: {:?}", appended.as_ref().map(|h| h.to_string()));
        let appended = appended.expect("B records a message under A's conversation");
        let _ = appended;

        let (a_ref, h) = (&a, hash.clone());
        eventually("A lists B's message under its conversation", 420, || async {
            let page: Result<Page, String> = a_ref
                .call("get_conversation_entries_page", EntriesPageInput { conversation_hash: h.clone(), after: None, limit: 10 })
                .await;
            matches!(page, Ok(ref p) if p.total == 1 && p.missing == 0)
        })
        .await;

        // (2) B starts a conversation AT A's key (the founding key as the
        // address for a write), and A lists two at its own anchor.
        let hash_b: ActionHash = b
            .call("start_conversation", StartConversationInput { cipher: b"ciphertext-b".to_vec(), nonce: vec![8u8; 24], anchor: Some(a.agent.clone()) })
            .await
            .expect("B starts a conversation at A's key");
        println!("  B started {} at A's key", hash_b);
        let a_ref2 = &a;
        eventually("A lists both conversations at its own key", 420, || async {
            let page: Result<Page, String> = a_ref2
                .call("get_conversations_page", ConversationsPageInput { anchor: None, before: None, limit: 10 })
                .await;
            matches!(page, Ok(ref p) if p.total == 2 && p.missing == 0)
        })
        .await;

        // (3) B deletes A's conversation from A's list: the list link goes,
        // a tombstone stays, and neither device lists it any more - the
        // entries themselves stay A's (author-only) until A cleans up.
        let deleted: u32 = b
            .call("delete_conversation_at", DeleteAtInput { conversation_hash: hash.clone(), anchor: Some(a.agent.clone()) })
            .await
            .expect("B tombstones A's conversation");
        println!("  B deleted {} entr{} of A's conversation (author-only: expected 0)", deleted, if deleted == 1 { "y" } else { "ies" });
        let (a_ref3, b_ref3, a_key3) = (&a, &b, a.agent.clone());
        eventually("neither device lists the tombstoned conversation", 420, || async {
            let on_a: Result<Page, String> = a_ref3
                .call("get_conversations_page", ConversationsPageInput { anchor: None, before: None, limit: 10 })
                .await;
            let on_b: Result<Page, String> = b_ref3
                .call("get_conversations_page", ConversationsPageInput { anchor: Some(a_key3.clone()), before: None, limit: 10 })
                .await;
            matches!((on_a, on_b), (Ok(ref pa), Ok(ref pb)) if pa.total == 1 && pb.total == 1)
        })
        .await;
    }
}
