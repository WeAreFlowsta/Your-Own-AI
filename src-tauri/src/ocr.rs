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
    let dir = app_handle
        .path()
        .resource_dir()
        .map_err(|e| format!("Failed to resolve resource dir: {e}"))?
        .join("resources")
        .join("pdfium");
    Ok(dir)
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

/// Whether the optional OCR models are installed (FE gate for the offer card).
#[tauri::command]
pub async fn is_ocr_ready(app_handle: AppHandle) -> Result<bool, String> {
    let models_dir = crate::llm::get_models_dir(&app_handle)?;
    Ok(ocr_models_present(&models_dir))
}

/// OCR a scanned PDF and return its text. Errors if the models aren't installed.
#[tauri::command]
pub async fn ocr_scanned_pdf(app_handle: AppHandle, file_path: String) -> Result<String, String> {
    let models_dir = crate::llm::get_models_dir(&app_handle)?;
    if !ocr_models_present(&models_dir) {
        return Err("OCR models not installed".to_string());
    }
    let lib_dir = pdfium_lib_dir(&app_handle)?;
    let pdf_path = PathBuf::from(&file_path);
    // pdfium + ocrs are CPU-bound and not async; run off the async runtime.
    let result = tauri::async_runtime::spawn_blocking(move || ocr_pdf(&pdf_path, &lib_dir, &models_dir))
        .await
        .map_err(|e| format!("OCR task failed: {e}"))?;
    if let Err(e) = &result {
        log::error!("[OCR] ocr_scanned_pdf failed: {e}");
    }
    result
}
