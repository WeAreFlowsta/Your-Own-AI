//! Transcript census — ground-truth audit of every transcript app.
//!
//! Connects to the running YOAI conductor, iterates EVERY installed
//! `transcript_*` app (no lineage logic), and lists each conversation with
//! its message count. The conductor accepts only the origin its start made,
//! written with its admin port in the open profile's conductor config, which
//! only this OS account can read. Run while the app is up:
//!
//!     cargo run --bin census -- <profile>/conductor/conductor-config.yaml
//!
//! Read-only: only zome reads (`get_all_conversations`,
//! `get_conversation_entries`) are issued.

use holochain_client::AdminWebsocket;
use holochain_types::prelude::ExternIO;

/// The admin port and origin a conductor config names.
fn port_and_origin(config: &str) -> Option<(u16, String)> {
    let value = |key: &str| {
        config.lines().find_map(|l| l.trim().strip_prefix(key).map(|v| v.trim().trim_matches('\'').to_string()))
    };
    Some((value("port:")?.parse().ok()?, value("allowed_origins:")?))
}

#[tokio::main]
async fn main() {
    let Some(path) = std::env::args().nth(1) else {
        eprintln!("usage: census <profile>/conductor/conductor-config.yaml");
        std::process::exit(2);
    };
    let config = std::fs::read_to_string(&path).unwrap_or_else(|e| {
        eprintln!("Cannot read {path}: {e}");
        std::process::exit(1);
    });
    let Some((admin_port, origin)) = port_and_origin(&config) else {
        eprintln!("{path} names no admin port and origin");
        std::process::exit(1);
    };
    let admin_ws = match AdminWebsocket::connect(format!("localhost:{}", admin_port), Some(origin.clone())).await {
        Ok(ws) => ws,
        Err(e) => {
            eprintln!("Cannot connect to admin port {admin_port} - is the app running? ({e})");
            std::process::exit(1);
        }
    };

    let apps = admin_ws.list_apps(None).await.expect("list_apps failed");
    let app_port = admin_ws
        .attach_app_interface(0, None, holochain_client::AllowedOrigins::Origins([origin.clone()].into_iter().collect()), None)
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
        let app_ws = match app_lib::dna::connect_app_websocket_as(admin_port, app_port, app_id, &origin).await {
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
