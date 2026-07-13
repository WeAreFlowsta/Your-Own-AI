//! Transcript census — ground-truth audit of every transcript app.
//!
//! Connects to the running YOAI conductor (admin port 4477), iterates
//! EVERY installed `transcript_*` app (no lineage logic), and lists each
//! conversation with its message count. Run while `npm run tauri dev`
//! is up:
//!
//!     cargo run --bin census
//!
//! Read-only: only zome reads (`get_all_conversations`,
//! `get_conversation_entries`) are issued.

use holochain_client::AdminWebsocket;
use holochain_types::prelude::ExternIO;

const ADMIN_PORT: u16 = 4477;

#[tokio::main]
async fn main() {
    let admin_ws = match AdminWebsocket::connect(
        format!("localhost:{}", ADMIN_PORT),
        Some("your-own-ai".to_string()),
    )
    .await
    {
        Ok(ws) => ws,
        Err(e) => {
            eprintln!("Cannot connect to admin port {ADMIN_PORT} — is the app running? ({e})");
            std::process::exit(1);
        }
    };

    let apps = admin_ws.list_apps(None).await.expect("list_apps failed");
    let app_port = admin_ws
        .attach_app_interface(0, None, holochain_client::AllowedOrigins::Any, None)
        .await
        .expect("attach_app_interface failed");

    let mut transcript_apps: Vec<String> = apps
        .iter()
        .filter(|a| a.installed_app_id.starts_with("transcript_"))
        .map(|a| a.installed_app_id.clone())
        .collect();
    transcript_apps.sort();

    println!("Found {} transcript apps\n", transcript_apps.len());

    let mut total_convs = 0usize;
    let mut total_msgs = 0usize;

    for app_id in &transcript_apps {
        let app_ws = match app_lib::dna::connect_app_websocket(ADMIN_PORT, app_port, app_id).await {
            Ok(ws) => ws,
            Err(e) => {
                println!("{app_id}\n  [unreachable: {e}]\n");
                continue;
            }
        };

        let payload = ExternIO::encode(()).unwrap();
        let result = match app_ws
            .call_zome(
                holochain_client::ZomeCallTarget::RoleName(app_lib::dna::ROLE_NAME.into()),
                "transcript".into(),
                "get_all_conversations".into(),
                payload,
            )
            .await
        {
            Ok(r) => r,
            Err(e) => {
                println!("{app_id}\n  [zome call failed: {e}]\n");
                continue;
            }
        };

        let records: Vec<holochain_types::prelude::Record> = match ExternIO::decode(&result) {
            Ok(r) => r,
            Err(e) => {
                println!("{app_id}\n  [decode failed: {e}]\n");
                continue;
            }
        };

        if records.is_empty() {
            println!("{app_id}\n  (no conversations)\n");
            continue;
        }

        println!("{app_id}");
        for record in &records {
            // Phase A: entries are EncryptedEntry {cipher, nonce} — the
            // census counts them but cannot read titles without the user
            // data key. Detect the shape and label honestly.
            let (title, started_at, name) = match record
                .entry()
                .as_option()
                .and_then(|e| e.as_app_entry())
                .and_then(|b| rmp_serde::from_slice::<serde_json::Value>(b.as_ref().bytes()).ok())
            {
                Some(v) if v.get("cipher").is_some() => {
                    ("(encrypted)".to_string(), 0, "?".to_string())
                }
                Some(v) => (
                    v["title"].as_str().unwrap_or("(untitled)").to_string(),
                    v["started_at"].as_i64().unwrap_or(0),
                    v["ai_personality_name"].as_str().unwrap_or("?").to_string(),
                ),
                None => ("(undecodable)".into(), 0, "?".into()),
            };

            // Count messages in this conversation.
            let conv_hash = record.action_address().clone();
            let payload = ExternIO::encode(conv_hash).unwrap();
            let n_msgs = match app_ws
                .call_zome(
                    holochain_client::ZomeCallTarget::RoleName(app_lib::dna::ROLE_NAME.into()),
                    "transcript".into(),
                    "get_conversation_entries".into(),
                    payload,
                )
                .await
            {
                Ok(r) => ExternIO::decode::<Vec<holochain_types::prelude::Record>>(&r)
                    .map(|v| v.len())
                    .unwrap_or(0),
                Err(_) => 0,
            };

            total_msgs += n_msgs;
            total_convs += 1;
            let when = chrono_lite(started_at);
            println!("  - [{name}] \"{title}\" — {n_msgs} messages ({when})");
        }
        println!();
    }

    println!("TOTAL: {total_convs} conversations, {total_msgs} messages across {} apps", transcript_apps.len());
}

/// Minimal epoch-micros → date string without a chrono dependency.
fn chrono_lite(micros: i64) -> String {
    if micros <= 0 {
        return "unknown date".into();
    }
    let secs = micros / 1_000_000;
    let days = secs / 86_400;
    // Days since 1970-01-01 → y-m-d (civil algorithm).
    let z = days + 719_468;
    let era = z / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    format!("{:04}-{:02}-{:02}", y, m, d)
}
