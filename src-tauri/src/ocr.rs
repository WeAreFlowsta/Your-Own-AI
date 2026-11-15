//! OCR for scanned PDFs.
//!
//! Renders each page with pdfium (a bundled native library that handles any PDF
//! codec, including the CCITT/JBIG2 B&W compression scanners produce) and
//! recognises the text with ocrs (pure-Rust, RTen runtime). This runs ONLY as the
//! fallback when a PDF has no extractable text layer. pdfium is bundled with the
//! app; the ocrs models are an optional download (Settings → Components).

use std::path::{Path, PathBuf};

use ocrs::{ImageSource, OcrEngine, OcrEngineParams};
use pdfium_render::prelude::*;
use rten::Model;
use tauri::{AppHandle, Manager};

pub const DETECTION_MODEL: &str = "text-detection.rten";
pub const RECOGNITION_MODEL: &str = "text-recognition.rten";

/// Directory holding the bundled pdfium library. Tauri bundles `resources/*`
/// under `<resource_dir>/resources/` (the conductor resolves the .happ the same
/// way — see lib.rs), so the path is `<resource_dir>/resources/pdfium/`.
fn pdfium_lib_dir(app_handle: &AppHandle) -> Result<PathBuf, String> {
    let data_dir = app_handle
        .path()
        .app_data_dir()
        .map_err(|e| format!("Failed to resolve data dir: {e}"))?;
    Ok(crate::resolve_resource_dir(app_handle, &data_dir).join("pdfium"))
}

/// Are both ocrs models downloaded?
pub fn ocr_models_present(models_dir: &Path) -> bool {
    models_dir.join(DETECTION_MODEL).exists() && models_dir.join(RECOGNITION_MODEL).exists()
}

/// OCR a scanned PDF: render each page → recognise → concatenate. The caller has
/// already established the PDF has no text layer.
pub fn ocr_pdf(pdf_path: &Path, lib_dir: &Path, models_dir: &Path) -> Result<String, String> {
    let detection = Model::load_file(models_dir.join(DETECTION_MODEL))
        .map_err(|e| format!("Failed to load OCR detection model: {e}"))?;
    let recognition = Model::load_file(models_dir.join(RECOGNITION_MODEL))
        .map_err(|e| format!("Failed to load OCR recognition model: {e}"))?;
    let engine = OcrEngine::new(OcrEngineParams {
        detection_model: Some(detection),
        recognition_model: Some(recognition),
        ..Default::default()
    })
    .map_err(|e| format!("Failed to initialise OCR engine: {e}"))?;

    let lib_name = Pdfium::pdfium_platform_library_name_at_path(lib_dir);
    log::info!("[OCR] loading pdfium from {}", lib_name.display());
    let pdfium = Pdfium::new(
        Pdfium::bind_to_library(&lib_name)
            .map_err(|e| format!("Failed to load bundled pdfium ({}): {e}", lib_name.display()))?,
    );
    let document = pdfium
        .load_pdf_from_file(pdf_path, None)
        .map_err(|e| format!("Failed to open PDF: {e}"))?;
    log::info!("[OCR] PDF has {} page(s)", document.pages().len());

    // ~200 DPI gives ocrs enough detail without ballooning memory.
    let render_config = PdfRenderConfig::new().set_target_width(2000);

    let mut out = String::new();
    for (idx, page) in document.pages().iter().enumerate() {
        let bitmap = page
            .render_with_config(&render_config)
            .map_err(|e| format!("Failed to render PDF page {}: {e}", idx + 1))?;
        let width = bitmap.width() as u32;
        let height = bitmap.height() as u32;
        // pdfium gives BGRA8; ocrs wants interleaved RGB8.
        let bgra = bitmap.as_raw_bytes();
        let expected = (width as usize) * (height as usize) * 4;
        log::info!(
            "[OCR] page {} bitmap {}x{} raw_len={} expected={} first_px={:?}",
            idx + 1,
            width,
            height,
            bgra.len(),
            expected,
            bgra.get(0..4)
        );
        let mut rgb = Vec::with_capacity((width as usize) * (height as usize) * 3);
        for px in bgra.chunks_exact(4) {
            rgb.push(px[2]);
            rgb.push(px[1]);
            rgb.push(px[0]);
        }
        let source = ImageSource::from_bytes(&rgb, (width, height))
            .map_err(|e| format!("OCR image prep failed: {e}"))?;
        let input = engine
            .prepare_input(source)
            .map_err(|e| format!("OCR input failed: {e}"))?;
        log::info!("[OCR] page {} recognising…", idx + 1);
        let text = engine
            .get_text(&input)
            .map_err(|e| format!("OCR recognition failed: {e}"))?;
        log::info!("[OCR] page {} recognised {} chars", idx + 1, text.len());
        let text = text.trim();
        if !text.is_empty() {
            if !out.is_empty() {
                out.push_str("\n\n");
            }
            out.push_str(text);
        }
    }
    log::info!("[OCR] total recognised {} chars", out.len());
    Ok(out)
}

/// Whether a scanned PDF can be read here: the document reader or the
/// built-in reader's models are installed (FE gate for the offer card).
#[tauri::command]
pub async fn is_ocr_ready(app_handle: AppHandle) -> Result<bool, String> {
    let models_dir = crate::llm::get_models_dir(&app_handle)?;
    Ok(crate::doc_reader::present(&models_dir) || ocr_models_present(&models_dir))
}

/// Read a scanned PDF and return its text: with the document reader when
/// it is installed (page by page, `document-reader-progress` events saying
/// which page), else with the built-in reader. If the document reader
/// fails and the built-in one is installed, that one reads the file, so a
/// scan is never left unread for the better reader's sake.
#[tauri::command]
pub async fn ocr_scanned_pdf(app_handle: AppHandle, file_path: String) -> Result<String, String> {
    use tauri::Emitter;
    let models_dir = crate::llm::get_models_dir(&app_handle)?;
    let builtin = ocr_models_present(&models_dir);
    let lib_dir = pdfium_lib_dir(&app_handle)?;
    let pdf_path = PathBuf::from(&file_path);
    if crate::doc_reader::present(&models_dir) {
        let read = async {
            let (bin, _) = crate::matrix::engine_binary(&app_handle)?;
            let card = crate::doc_reader::on_card(crate::matrix::free_vram_gb(&bin));
            let events = app_handle.clone();
            crate::doc_reader::read_pdf(&bin, &models_dir, lib_dir.clone(), pdf_path.clone(), card, move |done, of| {
                let _ = events.emit("document-reader-progress", serde_json::json!({ "done": done, "of": of }));
            })
            .await
        };
        match read.await {
            Ok(text) => return Ok(text),
            Err(e) if builtin => log::warn!("[reader] {e}; the built-in reader reads this file instead"),
            Err(e) => {
                log::error!("[reader] {e}");
                return Err(e);
            }
        }
    } else if !builtin {
        return Err("OCR models not installed".to_string());
    }
    // pdfium + ocrs are CPU-bound and not async; run off the async runtime.
    let result = tauri::async_runtime::spawn_blocking(move || ocr_pdf(&pdf_path, &lib_dir, &models_dir))
        .await
        .map_err(|e| format!("OCR task failed: {e}"))?;
    if let Err(e) = &result {
        log::error!("[OCR] ocr_scanned_pdf failed: {e}");
    }
    result
}

/// How much of a picture is text, by the built-in reader's detection step
/// alone (no reading): the number of words found and the share of the
/// picture their boxes cover. The quick check that tells a picture of a
/// document from an ordinary photo before the slow document reader runs.
pub struct TextShare {
    pub words: usize,
    pub covered: f64,
}

pub fn text_share(rgb: &[u8], width: u32, height: u32, detection_model: &Path) -> Result<TextShare, String> {
    let detection = Model::load_file(detection_model).map_err(|e| format!("Failed to load OCR detection model: {e}"))?;
    let engine = OcrEngine::new(OcrEngineParams { detection_model: Some(detection), ..Default::default() })
        .map_err(|e| format!("Failed to initialise OCR engine: {e}"))?;
    let source = ImageSource::from_bytes(rgb, (width, height)).map_err(|e| format!("OCR image prep failed: {e}"))?;
    let input = engine.prepare_input(source).map_err(|e| format!("OCR input failed: {e}"))?;
    let words = engine.detect_words(&input).map_err(|e| format!("OCR detection failed: {e}"))?;
    let area: f64 = words.iter().map(|r| (r.width() * r.height()) as f64).sum();
    Ok(TextShare { words: words.len(), covered: area / (width as f64 * height as f64).max(1.0) })
}

/// A picture attached in chat, read as a document when it is one: with the
/// document reader installed, the quick check counts the words in the
/// picture, and a picture that is mostly text is read into text. Anything
/// else (no reader, an ordinary photo, a picture this cannot open) answers
/// `document: false` and the picture goes on as a picture, as before.
#[tauri::command]
pub async fn read_picture_as_document(app_handle: AppHandle, file_path: String) -> Result<serde_json::Value, String> {
    use tauri::Emitter;
    let not_one = |words: usize| Ok(serde_json::json!({ "document": false, "words": words }));
    let models_dir = crate::llm::get_models_dir(&app_handle)?;
    let check = models_dir.join(crate::doc_reader::CHECK_FILE);
    if !crate::doc_reader::present(&models_dir) || !check.exists() {
        return not_one(0);
    }
    let (bin, _) = crate::matrix::engine_binary(&app_handle)?;
    let card = crate::doc_reader::on_card(crate::matrix::free_vram_gb(&bin));
    let events = app_handle.clone();
    let (words, text) = picture_as_document(&bin, &models_dir, PathBuf::from(&file_path), card, move || {
        let _ = events.emit("document-reader-progress", serde_json::json!({ "done": 0, "of": 1 }));
    })
    .await;
    let _ = app_handle.emit("document-reader-progress", serde_json::json!({ "done": 1, "of": 1 }));
    match text {
        Some(text) => Ok(serde_json::json!({ "document": true, "words": words, "text": text })),
        None => not_one(words),
    }
}

/// The quick check, then the reading when the picture is a document:
/// (words found, the text if it was read). `reading()` is called when the
/// slow part begins. A picture this cannot open, an ordinary photo, or a
/// reader that fails all answer `None`: the picture stays a picture.
pub async fn picture_as_document(bin: &Path, models_dir: &Path, path: PathBuf, card: bool, reading: impl Fn()) -> (usize, Option<String>) {
    let Ok(bytes) = std::fs::read(&path) else { return (0, None) };
    picture_bytes_as_document(bin, models_dir, bytes, card, reading).await
}

/// The same for a picture already in memory (one that arrived in a request).
pub async fn picture_bytes_as_document(bin: &Path, models_dir: &Path, bytes: Vec<u8>, card: bool, reading: impl Fn()) -> (usize, Option<String>) {
    let check = models_dir.join(crate::doc_reader::CHECK_FILE);
    let seen = tauri::async_runtime::spawn_blocking(move || -> Result<(usize, String), String> {
        let img = image::load_from_memory(&bytes).map_err(|_| "not a picture this can open".to_string())?;
        // A phone photo is far larger than the reader needs.
        let max = crate::doc_reader::PICTURE_MAX_SIDE;
        let img = if img.width() > max || img.height() > max { img.resize(max, max, image::imageops::FilterType::Triangle) } else { img };
        let rgb = img.to_rgb8();
        let share = text_share(rgb.as_raw(), rgb.width(), rgb.height(), &check)?;
        let bgra: Vec<u8> = rgb.pixels().flat_map(|p| [p[2], p[1], p[0], 255]).collect();
        Ok((share.words, crate::doc_reader::bgra_to_bmp_data_url(&bgra, rgb.width(), rgb.height())?))
    })
    .await;
    let Ok(Ok((words, picture))) = seen else { return (0, None) };
    if !crate::doc_reader::is_document(words) {
        return (words, None);
    }
    reading();
    let read = async { crate::doc_reader::Reader::start(bin, models_dir, card).await?.read(&picture).await };
    match read.await {
        Ok(text) if !text.trim().is_empty() => (words, Some(text)),
        Ok(_) => (words, None),
        Err(e) => {
            log::warn!("[reader] a picture was not read as a document: {e}");
            (words, None)
        }
    }
}

#[cfg(test)]
mod compare {
    /// Reads every PDF in `YOAI_OCR_COMPARE_DIR` with this reader (pdfium and
    /// ocrs, exactly as the app does for a scanned PDF) and writes the text
    /// beside it as `<name>.ocrs.txt`, with the seconds it took: one half of
    /// a reader comparison. `YOAI_MODELS_DIR` holds the two ocrs files.
    #[test]
    #[ignore]
    fn live_reader_compare_ocrs() {
        let Ok(dir) = std::env::var("YOAI_OCR_COMPARE_DIR") else { return };
        let home = std::env::var("HOME").unwrap_or_default();
        let models = std::env::var("YOAI_MODELS_DIR")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|_| std::path::Path::new(&home).join(".local/share/com.solar.yourowai/models"));
        let lib = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("resources/pdfium");
        let mut pdfs: Vec<_> = std::fs::read_dir(&dir).unwrap().flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|x| x == "pdf")).collect();
        pdfs.sort();
        assert!(!pdfs.is_empty(), "no PDF in {dir}");
        for pdf in pdfs {
            let t = std::time::Instant::now();
            let text = super::ocr_pdf(&pdf, &lib, &models).unwrap_or_else(|e| format!("ERROR: {e}"));
            let secs = t.elapsed().as_secs_f64();
            std::fs::write(pdf.with_extension("ocrs.txt"), &text).unwrap();
            eprintln!("[ocrs] {} | {secs:.1} s | {} chars", pdf.file_name().unwrap().to_string_lossy(), text.chars().count());
        }
    }

    /// Prints the text share of every PNG and JPEG in `YOAI_DOC_CHECK_DIR`:
    /// the figures the "is this picture a document" rule is set from.
    #[test]
    #[ignore]
    fn live_document_check() {
        let Ok(dir) = std::env::var("YOAI_DOC_CHECK_DIR") else { return };
        let home = std::env::var("HOME").unwrap_or_default();
        let models = std::path::Path::new(&home).join(".local/share/com.solar.yourowai/models");
        let mut files: Vec<_> = std::fs::read_dir(&dir).unwrap().flatten().map(|e| e.path()).collect();
        files.sort();
        for f in files {
            let Ok(img) = image::open(&f) else { continue };
            let rgb = img.to_rgb8();
            let t = std::time::Instant::now();
            let s = super::text_share(rgb.as_raw(), rgb.width(), rgb.height(), &models.join(super::DETECTION_MODEL)).unwrap();
            eprintln!("[check] {:<44} {:>4} words | {:>5.1}% covered | {:.1} s", f.file_name().unwrap().to_string_lossy(), s.words, s.covered * 100.0, t.elapsed().as_secs_f64());
        }
    }

    /// The picture path end to end on the files of `YOAI_DOC_CHECK_DIR`
    /// named in `YOAI_DOC_CHECK_ONLY` (comma-separated starts of names; all
    /// when unset): a `doc-` or `mix-letter`/`mix-screenshot` file must be
    /// read into text, a `pic-` or `mix-caption` file must stay a picture.
    /// `YOAI_MODELS_DIR` holds the reader's three files.
    #[tokio::test]
    #[ignore]
    async fn live_picture_as_document() {
        let Ok(dir) = std::env::var("YOAI_DOC_CHECK_DIR") else { return };
        let models = std::path::PathBuf::from(std::env::var("YOAI_MODELS_DIR").expect("YOAI_MODELS_DIR"));
        let only: Vec<String> = std::env::var("YOAI_DOC_CHECK_ONLY").map(|s| s.split(',').map(String::from).collect()).unwrap_or_default();
        let triple = if cfg!(target_os = "windows") { "x86_64-pc-windows-msvc.exe" }
            else if cfg!(target_os = "macos") { if cfg!(target_arch = "aarch64") { "aarch64-apple-darwin" } else { "x86_64-apple-darwin" } }
            else { "x86_64-unknown-linux-gnu" };
        let bin = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("bin").join(format!("{}-{triple}", crate::llm::BUNDLED_ENGINE));
        let card = crate::doc_reader::on_card(crate::matrix::free_vram_gb(&bin));
        let mut files: Vec<_> = std::fs::read_dir(&dir).unwrap().flatten().map(|e| e.path()).collect();
        files.sort();
        let mut wrong = Vec::new();
        for f in files {
            let name = f.file_name().unwrap().to_string_lossy().to_string();
            let picture = [".png", ".jpg", ".jpeg"].iter().any(|e| name.to_lowercase().ends_with(e));
            if !picture || (!only.is_empty() && !only.iter().any(|o| name.starts_with(o.as_str()))) {
                continue;
            }
            let t = std::time::Instant::now();
            let (words, text) = super::picture_as_document(&bin, &models, f.clone(), card, || {}).await;
            eprintln!("[picture] {name:<44} {words:>4} words | {} | {:.0} s", text.as_ref().map(|t| format!("read, {} chars", t.chars().count())).unwrap_or_else(|| "a picture".into()), t.elapsed().as_secs_f64());
            let want_text = name.starts_with("doc-") || name.starts_with("mix-letter") || name.starts_with("mix-screenshot");
            if want_text != text.is_some() {
                wrong.push(format!("{name}: {}", if want_text { "a document left unread" } else { "an ordinary picture read as a document" }));
            }
            if let Some(t) = text {
                std::fs::write(f.with_extension("picture.txt"), t).unwrap();
            }
        }
        assert!(wrong.is_empty(), "{}", wrong.join("\n"));
    }
}
