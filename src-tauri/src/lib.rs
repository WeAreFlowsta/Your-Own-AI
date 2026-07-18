// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod llm;                  // llama-server module
mod engine;               // optional inference engines (pinned tag; CUDA backend later)
mod ocr;                  // OCR for scanned PDFs (pdfium render + ocrs)
mod lair;                 // Holochain lair keystore management
mod conductor;            // Holochain conductor lifecycle
pub mod dna;              // DNA installation per AI personality (pub for bin/census)
pub mod flowsta;          // Sign in with Flowsta via Vault + online-model proxy
mod holochain;            // Multi-agent manager
mod transcript_crypto;    // User data key + per-user seed (Phase A privacy)
mod commands_holochain;   // Tauri commands for transcript operations
mod memory;               // Phase A persistent memory (encrypted profile facts)
mod transcript_memory;    // Per-AI episodic memory (embedded conversation turns)
mod gpu_safety;           // GPU crash-loop → CPU fallback (safe mode)
mod agent_bridge;         // Your Own AI Build coding agent over ACP (stdio JSON-RPC)
mod inference_server;     // OpenAI-compatible local server (external apps use custom AIs)
mod inference_memory;     // backend memory assembly for the inference server
mod router;               // Auto-mode model routing (offline / online+offline)
mod gguf;                  // minimal GGUF header reader (model metadata for fit)
mod fit;                   // VRAM/RAM fit grading per downloaded model
mod reset;                 // factory reset (wipe local AIs/transcripts/memory, relaunch)
mod vault_escrow;          // transcript-key escrow in the user's Flowsta Vault
mod vault_restore;         // replay conversations from the Vault backup onto this device
mod model_caps;            // capability registry (benchmark-informed scores)
mod process_ext;           // Windows: hide sidecar consoles + kill-on-close job; Linux: PDEATHSIG

use llm::LLMState;
use holochain::HolochainManager;
use tauri::{AppHandle, Emitter, Manager, RunEvent};
use std::fs;
use std::path::PathBuf;
use std::sync::Arc;

/// Resolve a sidecar binary path.
///
/// In production (bundled app), binaries are next to the main executable
/// (e.g. Contents/MacOS/holochain on macOS). In development, the official
/// release binaries in `src-tauri/binaries/` are preferred (same binaries
/// production ships — keeps dev and prod on the same conductor version),
/// falling back to PATH if absent.
pub fn resolve_sidecar_bin(name: &str) -> PathBuf {
    if cfg!(debug_assertions) {
        // Dev mode: prefer the target-suffixed official binary in
        // src-tauri/binaries/ (e.g. yourowai-holochain-x86_64-unknown-linux-gnu),
        // matching what CI bundles. Same pattern as ProofPoll.
        let binaries = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("binaries");
        let prefix = format!("{}-", name);
        if let Ok(entries) = std::fs::read_dir(&binaries) {
            for entry in entries.flatten() {
                if let Some(s) = entry.file_name().to_str() {
                    if s.starts_with(&prefix) {
                        return entry.path();
                    }
                }
            }
        }
        // Fallback: strip app-specific prefix to find a PATH binary
        // e.g. "yourowai-holochain" → "holochain"
        let base_name = name
            .strip_prefix("yourowai-")
            .unwrap_or(name);
        return PathBuf::from(base_name);
    }
    // Production: binary is next to our executable
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            let candidate = if cfg!(target_os = "windows") {
                dir.join(format!("{}.exe", name))
            } else {
                dir.join(name)
            };
            if candidate.exists() {
                return candidate;
            }
        }
    }
    // Fallback to PATH
    PathBuf::from(name)
}

// ============================================================================
// AI Thumbnail Management Functions
// ============================================================================

/// Get the thumbnails directory path
fn get_thumbnails_dir(app: &AppHandle) -> Result<PathBuf, String> {
    let app_data_dir = app
        .path()
        .app_data_dir()
        .map_err(|e| format!("Failed to get app data dir: {}", e))?;
    
    let thumbnails_dir = app_data_dir.join("thumbnails");
    
    // Create directory if it doesn't exist
    if !thumbnails_dir.exists() {
        fs::create_dir_all(&thumbnails_dir)
            .map_err(|e| format!("Failed to create thumbnails directory: {}", e))?;
    }
    
    Ok(thumbnails_dir)
}

/// Save an AI thumbnail to the filesystem
#[tauri::command]
async fn save_ai_thumbnail(
    app: AppHandle,
    ai_id: String,
    thumbnail_data: Vec<u8>,
) -> Result<String, String> {
    let thumbnails_dir = get_thumbnails_dir(&app)?;
    let thumbnail_path = thumbnails_dir.join(format!("{}.jpg", ai_id));
    
    fs::write(&thumbnail_path, thumbnail_data)
        .map_err(|e| format!("Failed to write thumbnail: {}", e))?;
    
    Ok(thumbnail_path
        .to_str()
        .ok_or("Invalid path")?
        .to_string())
}

/// Get an AI thumbnail from the filesystem
#[tauri::command]
async fn get_ai_thumbnail(
    app: AppHandle,
    ai_id: String,
) -> Result<Vec<u8>, String> {
    let thumbnails_dir = get_thumbnails_dir(&app)?;
    let thumbnail_path = thumbnails_dir.join(format!("{}.jpg", ai_id));
    
    if !thumbnail_path.exists() {
        return Err("Thumbnail not found".to_string());
    }
    
    fs::read(&thumbnail_path)
        .map_err(|e| format!("Failed to read thumbnail: {}", e))
}

/// Delete an AI thumbnail from the filesystem
#[tauri::command]
async fn delete_ai_thumbnail(
    app: AppHandle,
    ai_id: String,
) -> Result<(), String> {
    let thumbnails_dir = get_thumbnails_dir(&app)?;
    let thumbnail_path = thumbnails_dir.join(format!("{}.jpg", ai_id));
    
    if thumbnail_path.exists() {
        fs::remove_file(&thumbnail_path)
            .map_err(|e| format!("Failed to delete thumbnail: {}", e))?;
    }
    
    Ok(())
}

// ============================================================================
// File Context Functions — read local files for AI context
// ============================================================================

/// Plain text / code extensions (read directly as UTF-8)
const TEXT_EXTENSIONS: &[&str] = &[
    "txt", "md", "csv", "json", "xml", "yaml", "yml", "toml", "log", "ini", "cfg",
    "py", "js", "ts", "tsx", "jsx", "rs", "go", "java", "c", "cpp", "h", "hpp",
    "cs", "rb", "php", "swift", "kt", "scala", "sh", "bash", "zsh", "fish",
    "css", "scss", "less", "html", "htm", "sql", "r", "m", "lua", "pl", "ex",
    "exs", "hs", "clj", "erl", "elm", "vue", "svelte", "astro", "env", "gitignore",
    "dockerfile", "makefile", "cmake", "rtf",
];

/// Document extensions (need special parsing)
const DOCUMENT_EXTENSIONS: &[&str] = &[
    "pdf", "docx", "doc", "xlsx", "xls", "ods", "odt",
];

/// Max extracted text we'll keep (~256 KB of text). Larger content gets truncated.
const MAX_TEXT_CHARS: usize = 256 * 1024;

/// Extract text from a DOCX file (ZIP archive with word/document.xml)
fn extract_docx_text(path: &std::path::Path) -> Result<String, String> {
    let file = fs::File::open(path)
        .map_err(|e| format!("Failed to open DOCX: {}", e))?;
    let mut archive = zip::ZipArchive::new(file)
        .map_err(|e| format!("Failed to read DOCX archive: {}", e))?;

    let mut xml_content = String::new();
    {
        let mut doc_xml = archive.by_name("word/document.xml")
            .map_err(|_| "Not a valid DOCX file (missing word/document.xml)".to_string())?;
        std::io::Read::read_to_string(&mut doc_xml, &mut xml_content)
            .map_err(|e| format!("Failed to read document.xml: {}", e))?;
    }

    // Extract text from <w:t> tags and add paragraph breaks at </w:p>
    let mut text = String::new();
    let mut in_tag = false;
    let mut tag_name = String::new();
    let mut capturing = false;

    for ch in xml_content.chars() {
        if ch == '<' {
            in_tag = true;
            tag_name.clear();
        } else if ch == '>' {
            in_tag = false;
            if tag_name.contains("w:t") && !tag_name.starts_with('/') {
                capturing = true;
            } else if tag_name.contains("/w:t") {
                capturing = false;
            } else if tag_name.contains("/w:p") {
                text.push('\n');
            }
        } else if in_tag {
            tag_name.push(ch);
        } else if capturing {
            text.push(ch);
        }
    }

    let trimmed = text.trim().to_string();
    if trimmed.is_empty() {
        return Err("No text content found in DOCX file".to_string());
    }
    Ok(trimmed)
}

/// Extract text from an ODT file (ZIP archive with content.xml)
fn extract_odt_text(path: &std::path::Path) -> Result<String, String> {
    let file = fs::File::open(path)
        .map_err(|e| format!("Failed to open ODT: {}", e))?;
    let mut archive = zip::ZipArchive::new(file)
        .map_err(|e| format!("Failed to read ODT archive: {}", e))?;

    let mut xml_content = String::new();
    {
        let mut content_xml = archive.by_name("content.xml")
            .map_err(|_| "Not a valid ODT file (missing content.xml)".to_string())?;
        std::io::Read::read_to_string(&mut content_xml, &mut xml_content)
            .map_err(|e| format!("Failed to read content.xml: {}", e))?;
    }

    // Extract text content, add newlines at paragraph boundaries
    let mut text = String::new();
    let mut in_tag = false;
    let mut tag_name = String::new();

    for ch in xml_content.chars() {
        if ch == '<' {
            in_tag = true;
            tag_name.clear();
        } else if ch == '>' {
            in_tag = false;
            if tag_name.contains("/text:p") || tag_name.contains("/text:h") {
                text.push('\n');
            }
        } else if in_tag {
            tag_name.push(ch);
        } else if !in_tag {
            text.push(ch);
        }
    }

    let trimmed = text.trim().to_string();
    if trimmed.is_empty() {
        return Err("No text content found in ODT file".to_string());
    }
    Ok(trimmed)
}

/// Extract text from a PDF file
fn extract_pdf_text(path: &std::path::Path) -> Result<String, String> {
    let text = pdf_extract::extract_text(path)
        .map_err(|e| format!("Failed to extract text from PDF: {}", e))?;

    let trimmed = text.trim().to_string();
    if trimmed.is_empty() {
        return Err("No text content found in PDF (may be image-based/scanned)".to_string());
    }
    Ok(trimmed)
}

/// Extract text from an Excel/ODS spreadsheet
fn extract_spreadsheet_text(path: &std::path::Path) -> Result<String, String> {
    use calamine::{Reader, open_workbook_auto};

    let mut workbook = open_workbook_auto(path)
        .map_err(|e| format!("Failed to open spreadsheet: {}", e))?;

    let mut text = String::new();

    for sheet_name in workbook.sheet_names().to_vec() {
        if let Ok(range) = workbook.worksheet_range(&sheet_name) {
            if !text.is_empty() {
                text.push_str(&format!("\n\n--- Sheet: {} ---\n", sheet_name));
            } else {
                text.push_str(&format!("--- Sheet: {} ---\n", sheet_name));
            }

            for row in range.rows() {
                let cells: Vec<String> = row.iter().map(|cell| {
                    match cell {
                        calamine::Data::Empty => String::new(),
                        calamine::Data::String(s) => s.clone(),
                        calamine::Data::Float(f) => f.to_string(),
                        calamine::Data::Int(i) => i.to_string(),
                        calamine::Data::Bool(b) => b.to_string(),
                        calamine::Data::Error(e) => format!("#ERR:{:?}", e),
                        _ => String::from("[unsupported]"),
                    }
                }).collect();

                // Skip entirely empty rows
                if cells.iter().all(|c| c.is_empty()) { continue; }

                text.push_str(&cells.join("\t"));
                text.push('\n');
            }
        }
    }

    let trimmed = text.trim().to_string();
    if trimmed.is_empty() {
        return Err("No data found in spreadsheet".to_string());
    }
    Ok(trimmed)
}

/// Read an image into a base64 data URL for vision turns. Unlike
/// `read_file_for_context` (which extracts *text*), images are passed to the
/// model as image data via the multimodal content array. Capped at ~10 MB.
#[tauri::command]
async fn read_image_for_context(file_path: String) -> Result<serde_json::Value, String> {
    use base64::Engine;
    let path = std::path::Path::new(&file_path);
    if !path.is_file() {
        return Err("Not a file".to_string());
    }
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_lowercase();
    let mime = match ext.as_str() {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        // NOTE: no WebP — llama.cpp (stb_image) can't decode it; the model would
        // get no image. Convert to PNG first if WebP support is ever needed.
        "gif" => "image/gif",
        "bmp" => "image/bmp",
        _ => return Err(format!("Unsupported image type: .{}", ext)),
    };
    let bytes = std::fs::read(path).map_err(|e| format!("Failed to read image: {}", e))?;
    const MAX_IMAGE_BYTES: usize = 10 * 1024 * 1024;
    if bytes.len() > MAX_IMAGE_BYTES {
        return Err(format!(
            "Image too large ({} MB) — max 10 MB",
            bytes.len() / (1024 * 1024)
        ));
    }
    let size_bytes = bytes.len() as u64;
    let b64 = base64::engine::general_purpose::STANDARD.encode(&bytes);
    let filename = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("image")
        .to_string();
    Ok(serde_json::json!({
        "filename": filename,
        "data_url": format!("data:{};base64,{}", mime, b64),
        "size_bytes": size_bytes,
    }))
}

/// Read a file for AI context. Supports text files, DOCX, PDF, XLSX, ODT.
#[tauri::command]
async fn read_file_for_context(
    file_path: String,
) -> Result<serde_json::Value, String> {
    let path = std::path::Path::new(&file_path);

    if !path.exists() {
        return Err("File not found".to_string());
    }

    if !path.is_file() {
        return Err("Not a file".to_string());
    }

    let ext = path.extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_lowercase();

    let filename = path.file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("unknown")
        .to_string();

    let known_names = ["makefile", "dockerfile", "gemfile", "rakefile", "procfile"];
    let is_text = TEXT_EXTENSIONS.contains(&ext.as_str())
        || known_names.contains(&filename.to_lowercase().as_str());
    let is_document = DOCUMENT_EXTENSIONS.contains(&ext.as_str());

    if !is_text && !is_document {
        return Err(format!(
            "Unsupported file type: .{}. Supported: text, code, PDF, DOCX, XLSX, ODT.",
            ext
        ));
    }

    let metadata = fs::metadata(path)
        .map_err(|e| format!("Failed to read file metadata: {}", e))?;
    let size_bytes = metadata.len();

    // Extract content based on file type
    let content = if is_document {
        match ext.as_str() {
            "docx" | "doc" => extract_docx_text(path)?,
            "odt" => extract_odt_text(path)?,
            "pdf" => extract_pdf_text(path)?,
            "xlsx" | "xls" | "ods" => extract_spreadsheet_text(path)?,
            _ => return Err(format!("Document type .{} not yet supported", ext)),
        }
    } else {
        fs::read_to_string(path)
            .map_err(|e| format!("Failed to read file (may be binary): {}", e))?
    };

    // Truncate if too large
    let truncated = content.len() > MAX_TEXT_CHARS;
    let final_content = if truncated {
        let truncated_content: String = content.chars().take(MAX_TEXT_CHARS).collect();
        format!("{}\n\n(Content truncated — showing first {} of {} characters)", truncated_content, MAX_TEXT_CHARS, content.len())
    } else {
        content
    };

    let estimated_tokens = (final_content.len() as f64 / 4.0).ceil() as u64;

    Ok(serde_json::json!({
        "filename": filename,
        "path": file_path,
        "size_bytes": size_bytes,
        "content": final_content,
        "truncated": truncated,
        "estimated_tokens": estimated_tokens,
    }))
}

/// Get the current model's context window size in tokens
#[tauri::command]
async fn get_context_window_size() -> Result<u64, String> {
    let sys = sysinfo::System::new_with_specifics(
        sysinfo::RefreshKind::nothing().with_memory(sysinfo::MemoryRefreshKind::everything()),
    );
    let total_ram_gb = sys.total_memory() as f64 / (1024.0 * 1024.0 * 1024.0);
    let ctx = if total_ram_gb >= 32.0 {
        16384
    } else if total_ram_gb >= 12.0 {
        8192
    } else {
        4096
    };
    Ok(ctx)
}

// ============================================================================
// Main Application Entry Point
// ============================================================================

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_store::Builder::default().build())
        .plugin(tauri_plugin_http::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_window_state::Builder::new()
            // Remembers size/position/maximized on Windows, macOS, and Linux/X11.
            // NOTE: restore is a no-op on GNOME Wayland (the compositor owns window
            // geometry; saved size isn't honoured on creation, position can't be set
            // at all). Accepted limitation — see apps/your-own-ai/building.md.
            .with_state_flags(tauri_plugin_window_state::StateFlags::all())
            .build())
        .plugin(tauri_plugin_log::Builder::new()
            .level(log::LevelFilter::Info)
            .build())
        .manage(LLMState::new())
        .manage(agent_bridge::AgentBridgeState::new())
        .invoke_handler(tauri::generate_handler![
            flowsta::flowsta_vault_status,
            flowsta::vault_sign,
            flowsta::verify_pack_signature,
            flowsta::flowsta_sign_in,
            flowsta::flowsta_sign_out,
            flowsta::flowsta_session,
            flowsta::flowsta_link_url,
            flowsta::flowsta_account_url,
            flowsta::list_online_models,
            flowsta::vault_sign_document,
            vault_escrow::vault_escrow_sync,
            vault_escrow::vault_escrow_restore,
            vault_escrow::vault_escrow_keep_local,
            vault_escrow::vault_backup_now,
            vault_restore::vault_restore_conversations,
            vault_restore::vault_restore_pending,
            vault_restore::memory_reembed_pending,
            vault_restore::memory_reembed_done,
            llm::get_system_info,
            llm::list_local_models,
            engine::engine_status,
            engine::download_cuda_engine,
            engine::remove_cuda_engine,
            engine::set_external_engine,
            engine::external_engine_info,
            engine::remove_external_engine,
            router::route_model,
            router::is_medical_query,
            router::routing_specialist_tasks,
            fit::assess_model_fit,
            reset::reset_to_defaults,
            llm::download_model,
            llm::download_status,
            ocr::is_ocr_ready,
            ocr::ocr_scanned_pdf,
            llm::delete_model,
            llm::get_models_directory,
            llm::is_model_downloaded,
            llm::load_model,
            llm::ensure_vision,
            llm::is_model_too_big,
            llm::get_current_model,
            llm::is_llama_server_running,
            llm::is_llama_server_ready,
            llm::is_vision_ready,
            llm::find_vision_model,
            llm::start_llama_server,
            llm::stop_llama_server,
            llm::embed_texts,
            llm::stop_embedding_server,
            llm::utility_chat,
            llm::stop_utility_server,
            llm::kill_port_8080,
            agent_bridge::start_build_agent,
            agent_bridge::send_agent_prompt,
            agent_bridge::cancel_agent_turn,
            agent_bridge::respond_agent_permission,
            agent_bridge::stop_build_agent,
            agent_bridge::build_agent_status,
            agent_bridge::path_is_dir,
            agent_bridge::path_is_file,
            llm::stream_chat_completion,
            llm::cancel_chat_completion,
            save_ai_thumbnail,
            get_ai_thumbnail,
            delete_ai_thumbnail,
            read_file_for_context,
            read_image_for_context,
            get_context_window_size,
            // Holochain transcript commands
            commands_holochain::provision_ai_agent,
            commands_holochain::provision_all_agents,
            commands_holochain::start_conversation,
            commands_holochain::record_transcript_entry,
            commands_holochain::record_grounding_annotation,
            commands_holochain::get_conversations,
            commands_holochain::get_conversation_transcript,
            commands_holochain::get_ai_holochain_status,
            commands_holochain::purge_ai_app,
            commands_holochain::export_transcript_recovery,
            commands_holochain::save_text_download,
            memory::get_memory_facts,
            memory::save_memory_facts,
            transcript_memory::get_transcript_embeddings,
            transcript_memory::save_transcript_embeddings,
            gpu_safety::gpu_safe_mode_status,
            gpu_safety::gpu_retry,
        ])
        .setup(|app| {
            // Surface which online-model endpoint is in use (prod vs a dev
            // override) so it's obvious at a glance in dev logs.
            log::info!("[flowsta] online-model proxy: {}", flowsta::proxy_url());

            // Keep that proxy warm for signed-in users (cold start ≈ 3.5s).
            flowsta::spawn_proxy_keepalive(app.handle().clone());

            // Start the OpenAI-compatible inference server (external apps use
            // the user's custom AIs). Best-effort; logs and continues on bind
            // failure.
            inference_server::spawn(app.handle().clone());

            // Start bundled llama-server on app startup
            let app_handle = app.handle().clone();

            tauri::async_runtime::spawn(async move {
                let llm_state = app_handle.state::<LLMState>();

                // Check if server is already running
                match llm::is_llama_server_running().await {
                    Ok(true) => {
                        println!("[Setup] llama-server already running");
                    }
                    Ok(false) => {
                        println!("[Setup] Starting bundled llama-server...");

                        // Start without a model (model will be loaded when user selects one)
                        match llm::start_llama_server(app_handle.clone(), llm_state, None, false).await {
                            Ok(_) => {
                                println!("[Setup] llama-server started successfully on port 8080");
                            }
                            Err(e) => {
                                eprintln!("[Setup] Failed to start llama-server: {}", e);
                                eprintln!("[Setup] App will continue, but local AI will not work");
                            }
                        }
                    }
                    Err(e) => {
                        eprintln!("[Setup] Error checking llama-server: {}", e);
                    }
                }
            });

            // Reconnect to Vault on launch if a prior link was wiped (e.g. Vault
            // was reset): re-link so YOAI reappears in Vault's connected apps,
            // matching ProofPoll. No-op when signed out; retries briefly so an
            // unlock shortly after launch still reconnects.
            let link_app_handle = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                flowsta::reconcile_vault_link_on_launch(link_app_handle.clone()).await;
                // With the link settled, reconcile the transcript-key escrow
                // too (write-if-absent only; conflicts wait for the user in
                // Settings). Covers signed-in users who never open Settings.
                let status = vault_escrow::vault_escrow_sync(link_app_handle).await;
                log::info!("[escrow] launch sync: {}", status.state);
            });

            // Register Holochain state synchronously (manager filled when conductor starts)
            let hc_state = Arc::new(commands_holochain::HolochainState::new());
            app.manage(hc_state.clone());

            // Start Holochain conductor in background
            let hc_app_handle = app.handle().clone();
            let data_dir = app
                .path()
                .app_data_dir()
                .expect("Failed to get app data dir");
            let resource_dir = app
                .path()
                .resource_dir()
                .expect("Failed to get resource dir")
                .join("resources");

            tauri::async_runtime::spawn(async move {
                log::info!("Starting Holochain conductor...");
                let passphrase = "your-own-ai-local";

                // Phase A: user data key + per-user network seed,
                // generated on first launch (standalone — no account).
                let recovery = match transcript_crypto::ensure_recovery_material(&data_dir) {
                    Ok(r) => r,
                    Err(e) => {
                        log::error!("Failed to prepare recovery material: {}", e);
                        return;
                    }
                };

                match conductor::start_holochain(
                    hc_app_handle.clone(),
                    data_dir,
                    passphrase.to_string(),
                )
                .await
                {
                    Ok(startup) => {
                        let manager = HolochainManager::new(startup, resource_dir, recovery);

                        // Phase A fresh start: remove pre-versioning apps.
                        if let Err(e) = dna::cleanup_legacy_apps(manager.handle.admin_port).await {
                            log::warn!("Legacy app cleanup failed: {}", e);
                        }

                        // Reconnect any agents from previous sessions.
                        if let Err(e) = manager.reconnect_existing_agents().await {
                            log::warn!("Failed to reconnect existing agents: {}", e);
                        }

                        // Fill the OnceCell
                        let _ = hc_state.manager.set(manager);
                        log::info!("Holochain conductor ready");
                    }
                    Err(e) => {
                        log::error!("Failed to start Holochain conductor: {}", e);
                        log::error!("Transcripts will not be recorded this session");
                        let _ = hc_app_handle.emit(
                            "conductor-status",
                            conductor::ConductorStatus::Error {
                                message: e.clone(),
                            },
                        );
                    }
                }
            });

            Ok(())
        })
        .build(tauri::generate_context!())
        .expect("error while running tauri application")
        .run(|app_handle, event| {
            // Handle app exit - stop llama-server
            if let RunEvent::ExitRequested { .. } = event {
                // Clean exit → clear the GPU crash sentinel so this run isn't
                // counted as a hard crash by safe mode.
                gpu_safety::mark_clean_exit(app_handle);
                println!("[Exit] Stopping llama-server...");
                agent_bridge::kill_on_exit(app_handle);
                let llm_state = app_handle.state::<LLMState>();
                
                // Use blocking lock since we're in sync context
                let mut server_process = llm_state.server_process.blocking_lock();
                let mut is_running = llm_state.is_server_running.blocking_lock();
                
                if let Some(child) = server_process.take() {
                    if let Err(e) = child.kill() {
                        eprintln!("[Exit] Failed to stop llama-server: {}", e);
                    } else {
                        println!("[Exit] llama-server stopped");
                    }
                    *is_running = false;
                }

                // Stop the embedding server too, if it was started.
                if let Some(child) = llm_state.embed_process.blocking_lock().take() {
                    let _ = child.kill();
                    *llm_state.embed_running.blocking_lock() = false;
                    println!("[Exit] embedding server stopped");
                }

                // Stop the utility server too, if it was started.
                if let Some(child) = llm_state.util_process.blocking_lock().take() {
                    let _ = child.kill();
                    *llm_state.util_running.blocking_lock() = false;
                    println!("[Exit] utility server stopped");
                }

                // Stop Holochain conductor + lair by killing processes via PID
                println!("[Exit] Stopping Holochain conductor...");
                let hc_state = app_handle.state::<Arc<commands_holochain::HolochainState>>();
                if let Some(manager) = hc_state.manager.get() {
                    let conductor_pid = manager.handle.conductor_child.id();
                    let lair_pid = manager.handle.lair_child.id();
                    let _ = std::process::Command::new("kill").arg(conductor_pid.to_string()).output();
                    let _ = std::process::Command::new("kill").arg(lair_pid.to_string()).output();
                    println!("[Exit] Holochain stopped (pids: {}, {})", conductor_pid, lair_pid);
                }
            }
        });
}