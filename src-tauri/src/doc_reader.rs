//! The document reader: an optional component that reads a picture of a
//! document (a scanned page, a photo of a letter, a screenshot of a table)
//! into text. It is a small vision model (MiniCPM-OCR 1.0, Apache 2.0) on
//! the bundled engine, started for one reading job and stopped after it.
//!
//! Better than the built-in reader (`ocr.rs`) on photos, tables and forms,
//! where that one misreads figures and pulls columns apart; about level on
//! clean dense print; many times slower (a page takes from 20 seconds with
//! the picture read on the card to over a minute on the processor). So it
//! is an option a person installs, and the built-in reader stays the
//! default and the fallback. Measured 2026-11-16 on six made-up documents.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

/// The model and its vision file as they sit in the models folder. The
/// vision file is saved under the model's own name (the maker ships it as
/// `mmproj-model-f16-4x.gguf`).
pub const MODEL_FILE: &str = "MiniCPM-OCR-1_0-Q4_K_M.gguf";
pub const VISION_FILE: &str = "MiniCPM-OCR-1_0-mmproj-f16.gguf";
/// The quick check's model: the built-in reader's text detection, kept as
/// this component's own file so removing either component leaves the other
/// whole.
pub const CHECK_FILE: &str = "document-check.rten";
/// A picture with at least this many words found in it is a document.
/// Measured 2026-11-16: nine documents and screenshots had 62 words or
/// more, ten ordinary pictures 18 or fewer, a picture with a caption 12.
pub const DOCUMENT_WORDS: usize = 40;
/// The longest side a picture is read at.
pub const PICTURE_MAX_SIDE: u32 = 1800;

pub fn is_document(words_found: usize) -> bool {
    words_found >= DOCUMENT_WORDS
}

/// The model's own instruction for plain reading (its SDK's default).
const READ_PROMPT: &str = "Text Recognition:";
/// Width a PDF page is drawn at for the reader: A4 at 150 dpi, the size
/// the comparison was run at.
pub const PAGE_WIDTH: i32 = 1240;
/// Free memory on the card the reader needs to read the picture there too
/// (the model 0.5 GB, the vision file 0.9 GB, their working room).
const CARD_ROOM_GB: f64 = 2.5;
const PAGE_TIMEOUT: Duration = Duration::from_secs(600);

/// Are both files downloaded?
pub fn present(models_dir: &Path) -> bool {
    models_dir.join(MODEL_FILE).exists() && models_dir.join(VISION_FILE).exists()
}

/// The reader's model file is a component, never a chat model: it answers
/// only its own reading instructions.
pub fn is_reader_file(filename: &str) -> bool {
    filename.to_lowercase().starts_with("minicpm-ocr")
}

/// Where the picture is read: on the card when it has the room, else on
/// the processor (slower, and it always works).
pub fn on_card(free_vram_gb: Option<f64>) -> bool {
    free_vram_gb.is_some_and(|f| f >= CARD_ROOM_GB)
}

struct KillOnDrop(std::process::Child);
impl Drop for KillOnDrop {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

/// One reading job at a time: the reader has one port, and two engines
/// side by side would halve each other's speed. A second job waits.
static ONE_JOB: tokio::sync::Semaphore = tokio::sync::Semaphore::const_new(1);

/// The reader, running for one job. Dropping it stops the engine and lets
/// the next job start.
pub struct Reader {
    _engine: KillOnDrop,
    _job: tokio::sync::SemaphorePermit<'static>,
    port: u16,
    pub card: bool,
}

impl Reader {
    /// Start the engine with the reader's model. With `card`, a start that
    /// fails is tried once more on the processor.
    pub async fn start(bin: &Path, models_dir: &Path, card: bool) -> Result<Reader, String> {
        if !present(models_dir) {
            return Err("The document reader is not installed".into());
        }
        let job = ONE_JOB.acquire().await.map_err(|_| "The document reader is closing".to_string())?;
        let engine = match Self::start_at(bin, models_dir, card).await {
            Err(e) if card => {
                log::warn!("[reader] did not start on the card ({e}); on the processor instead");
                (Self::start_at(bin, models_dir, false).await?, false)
            }
            r => (r?, card),
        };
        Ok(Reader { _engine: engine.0 .0, _job: job, port: engine.0 .1, card: engine.1 })
    }

    async fn start_at(bin: &Path, models_dir: &Path, card: bool) -> Result<(KillOnDrop, u16), String> {
        let port = crate::ports::reader();
        let mut args: Vec<String> = [
            "--model", MODEL_FILE, "--mmproj", VISION_FILE,
            "--host", "127.0.0.1", "--no-webui", "--reasoning", "off",
            "--ctx-size", "8192", "--parallel", "1",
        ].iter().map(|s| s.to_string()).collect();
        args.extend(["--port".to_string(), port.to_string()]);
        // The engine answers only this app, as the chat engine does.
        args.extend(["--api-key".to_string(), crate::llm::local_api_key().to_string()]);
        if card {
            args.extend(["-ngl".to_string(), "99".to_string()]);
        } else {
            args.extend(["--device", "none", "-ngl", "0", "--no-mmproj-offload"].iter().map(|s| s.to_string()));
        }
        let mut cmd = std::process::Command::new(bin);
        cmd.args(&args).current_dir(models_dir).stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null());
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
        }
        let mut engine = KillOnDrop(cmd.spawn().map_err(|e| format!("The document reader could not start: {e}"))?);
        let client = crate::llm::local_http();
        let deadline = Instant::now() + Duration::from_secs(180);
        loop {
            if let Ok(Some(st)) = engine.0.try_wait() {
                return Err(format!("The document reader stopped while loading ({st})"));
            }
            if let Ok(r) = client.get(format!("http://127.0.0.1:{port}/health")).send().await {
                if r.status().is_success() {
                    log::info!("[reader] ready on the {}", if card { "card" } else { "processor" });
                    return Ok((engine, port));
                }
            }
            if Instant::now() >= deadline {
                return Err("The document reader did not load in 3 minutes".into());
            }
            tokio::time::sleep(Duration::from_millis(400)).await;
        }
    }

    /// One picture (a `data:` address the engine can open: PNG, JPEG or
    /// BMP) -> the text on it.
    pub async fn read(&self, image_data_url: &str) -> Result<String, String> {
        let body = serde_json::json!({
            "messages": [{"role": "user", "content": [
                {"type": "image_url", "image_url": {"url": image_data_url}},
                {"type": "text", "text": READ_PROMPT},
            ]}],
            "max_tokens": 4096, "temperature": 0.0, "stream": false,
        });
        let v: serde_json::Value = crate::llm::local_http()
            .post(format!("http://127.0.0.1:{}/v1/chat/completions", self.port))
            .bearer_auth(crate::llm::local_api_key())
            .json(&body)
            .timeout(PAGE_TIMEOUT)
            .send()
            .await
            .map_err(|e| format!("The document reader did not answer: {}", if e.is_timeout() { "it took too long" } else { "the request failed" }))?
            .json()
            .await
            .map_err(|_| "The document reader's answer could not be read".to_string())?;
        v["choices"][0]["message"]["content"]
            .as_str()
            .map(|s| s.trim().to_string())
            .ok_or_else(|| "The document reader gave no text".to_string())
    }
}

/// Top-down BGRA pixels (what pdfium draws) as a 24-bit BMP in a `data:`
/// address: a picture format the engine opens, written here without an
/// image library.
pub fn bgra_to_bmp_data_url(bgra: &[u8], width: u32, height: u32) -> Result<String, String> {
    use base64::Engine as _;
    let (w, h) = (width as usize, height as usize);
    if w == 0 || h == 0 || bgra.len() < w * h * 4 {
        return Err("The page could not be drawn".into());
    }
    let row = (w * 3 + 3) & !3;
    let size = 54 + row * h;
    let mut out = Vec::with_capacity(size);
    out.extend_from_slice(b"BM");
    out.extend_from_slice(&(size as u32).to_le_bytes());
    out.extend_from_slice(&[0; 4]);
    out.extend_from_slice(&54u32.to_le_bytes());
    out.extend_from_slice(&40u32.to_le_bytes());
    out.extend_from_slice(&(width as i32).to_le_bytes());
    out.extend_from_slice(&(height as i32).to_le_bytes()); // positive: rows bottom-up
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&24u16.to_le_bytes());
    out.extend_from_slice(&[0; 4]);
    out.extend_from_slice(&((row * h) as u32).to_le_bytes());
    out.extend_from_slice(&[0; 16]);
    for y in (0..h).rev() {
        let line = &bgra[y * w * 4..(y + 1) * w * 4];
        for px in line.chunks_exact(4) {
            out.extend_from_slice(&px[..3]); // BMP keeps blue, green, red
        }
        out.resize(out.len() + (row - w * 3), 0);
    }
    Ok(format!("data:image/bmp;base64,{}", base64::engine::general_purpose::STANDARD.encode(out)))
}

/// How many pages a PDF has (0 for one that cannot be opened is an error).
pub fn pdf_page_count(pdf_path: &Path, pdfium_lib_dir: &Path) -> Result<usize, String> {
    use pdfium_render::prelude::*;
    let lib = Pdfium::pdfium_platform_library_name_at_path(pdfium_lib_dir);
    let pdfium = Pdfium::new(Pdfium::bind_to_library(&lib).map_err(|e| format!("Failed to load bundled pdfium: {e}"))?);
    let document = pdfium.load_pdf_from_file(pdf_path, None).map_err(|e| format!("Failed to open PDF: {e}"))?;
    Ok(document.pages().len() as usize)
}

/// Every page of a PDF drawn for the reader, as `data:` addresses.
pub fn pdf_pages(pdf_path: &Path, pdfium_lib_dir: &Path) -> Result<Vec<String>, String> {
    use pdfium_render::prelude::*;
    let lib = Pdfium::pdfium_platform_library_name_at_path(pdfium_lib_dir);
    let pdfium = Pdfium::new(Pdfium::bind_to_library(&lib).map_err(|e| format!("Failed to load bundled pdfium: {e}"))?);
    let document = pdfium.load_pdf_from_file(pdf_path, None).map_err(|e| format!("Failed to open PDF: {e}"))?;
    let config = PdfRenderConfig::new().set_target_width(PAGE_WIDTH);
    let mut pages = Vec::new();
    for (i, page) in document.pages().iter().enumerate() {
        let bitmap = page.render_with_config(&config).map_err(|e| format!("Failed to render PDF page {}: {e}", i + 1))?;
        pages.push(bgra_to_bmp_data_url(&bitmap.as_raw_bytes(), bitmap.width() as u32, bitmap.height() as u32)?);
    }
    Ok(pages)
}

/// A scanned PDF read page by page. `progress(done, of)` is called before
/// each page and once at the end.
pub async fn read_pdf(
    bin: &Path,
    models_dir: &Path,
    pdfium_lib_dir: PathBuf,
    pdf_path: PathBuf,
    card: bool,
    progress: impl Fn(usize, usize),
) -> Result<String, String> {
    let pages = tauri::async_runtime::spawn_blocking(move || pdf_pages(&pdf_path, &pdfium_lib_dir))
        .await
        .map_err(|e| format!("The pages could not be drawn: {e}"))??;
    let reader = Reader::start(bin, models_dir, card).await?;
    let mut out = String::new();
    for (i, page) in pages.iter().enumerate() {
        progress(i, pages.len());
        let started = Instant::now();
        let text = reader.read(page).await?;
        log::info!("[reader] page {} of {}: {} characters in {:.0} s", i + 1, pages.len(), text.chars().count(), started.elapsed().as_secs_f64());
        if !text.is_empty() {
            if !out.is_empty() {
                out.push_str("\n\n");
            }
            out.push_str(&text);
        }
    }
    progress(pages.len(), pages.len());
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_page_becomes_a_bottom_up_24_bit_bmp() {
        // 2 x 2: top row red, green; bottom row blue, white (BGRA).
        let bgra = [0, 0, 255, 255, 0, 255, 0, 255, 255, 0, 0, 255, 255, 255, 255, 255];
        let url = bgra_to_bmp_data_url(&bgra, 2, 2).unwrap();
        use base64::Engine as _;
        let bmp = base64::engine::general_purpose::STANDARD.decode(url.strip_prefix("data:image/bmp;base64,").unwrap()).unwrap();
        assert_eq!(&bmp[..2], b"BM");
        assert_eq!(bmp.len(), 54 + 8 * 2, "each row of two pixels is padded to eight bytes");
        assert_eq!(&bmp[54..60], &[255, 0, 0, 255, 255, 255], "the bottom row first: blue, white");
        assert_eq!(&bmp[62..68], &[0, 0, 255, 0, 255, 0], "then the top row: red, green");
        assert!(bgra_to_bmp_data_url(&bgra[..8], 2, 2).is_err());
    }

    #[test]
    fn the_reader_takes_the_card_only_with_room() {
        assert!(on_card(Some(4.0)));
        assert!(!on_card(Some(1.1)));
        assert!(!on_card(None));
        assert!(is_document(62) && !is_document(18));
        assert!(is_reader_file("MiniCPM-OCR-1_0-Q4_K_M.gguf") && !is_reader_file("gemma-4-E2B-it-Q4_K_M.gguf"));
    }

    /// Reads every PDF in `YOAI_OCR_COMPARE_DIR` with the document reader
    /// and writes the text beside it as `<name>.reader.txt`. With a
    /// `truth.json` there (name -> the text on the page), every FIGURE on a
    /// page must come out: a reader that changes a number is worse than
    /// none. `YOAI_MODELS_DIR` holds the reader's two files.
    #[tokio::test]
    #[ignore]
    async fn live_reader_reads_the_documents() {
        let Ok(dir) = std::env::var("YOAI_OCR_COMPARE_DIR") else { return };
        let home = std::env::var("HOME").unwrap_or_default();
        let models = std::env::var("YOAI_MODELS_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|_| Path::new(&home).join(".local/share/com.solar.yourowai/models"));
        let triple = if cfg!(target_os = "windows") { "x86_64-pc-windows-msvc.exe" }
            else if cfg!(target_os = "macos") { if cfg!(target_arch = "aarch64") { "aarch64-apple-darwin" } else { "x86_64-apple-darwin" } }
            else { "x86_64-unknown-linux-gnu" };
        let bin = Path::new(env!("CARGO_MANIFEST_DIR")).join("bin").join(format!("{}-{triple}", crate::llm::BUNDLED_ENGINE));
        let lib = Path::new(env!("CARGO_MANIFEST_DIR")).join("resources/pdfium");
        let card = on_card(crate::matrix::free_vram_gb(&bin));
        let truth: serde_json::Value = std::fs::read_to_string(Path::new(&dir).join("truth.json")).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default();
        let mut pdfs: Vec<_> = std::fs::read_dir(&dir).unwrap().flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|x| x == "pdf")).collect();
        pdfs.sort();
        assert!(!pdfs.is_empty(), "no PDF in {dir}");
        let mut wrong = Vec::new();
        for pdf in pdfs {
            let name = pdf.file_stem().unwrap().to_string_lossy().to_string();
            let t = Instant::now();
            let text = read_pdf(&bin, &models, lib.clone(), pdf.clone(), card, |_, _| {}).await.unwrap();
            eprintln!("[reader] {name} | {} | {:.0} s | {} chars", if card { "card" } else { "processor" }, t.elapsed().as_secs_f64(), text.chars().count());
            std::fs::write(pdf.with_extension("reader.txt"), &text).unwrap();
            if let Some(page) = truth[&name].as_str() {
                let figures = |s: &str| -> Vec<String> {
                    s.split(|c: char| !(c.is_ascii_digit() || matches!(c, '.' | ',' | ':' | '/' | '-')))
                        .map(|w| w.trim_matches(|c: char| !c.is_ascii_digit()).to_string())
                        .filter(|w| !w.is_empty())
                        .collect()
                };
                let got = figures(&text);
                for f in figures(page) {
                    if !got.contains(&f) {
                        wrong.push(format!("{name}: the figure {f} is on the page and not in the text"));
                    }
                }
            }
        }
        assert!(wrong.is_empty(), "{}", wrong.join("\n"));
    }
}
