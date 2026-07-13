# Bundled pdfium

The pdfium library (renders scanned PDF pages for OCR) is fetched per-platform here
by `scripts/fetch-pdfium.sh` and bundled as a Tauri resource. The binary itself is
**not committed** (~6–10 MB; see `.gitignore`) — run the script before building:

```
./scripts/fetch-pdfium.sh            # auto-detect this host
./scripts/fetch-pdfium.sh mac-arm64  # or a specific target (CI runs per-platform)
```

At runtime the app loads `libpdfium.{so,dylib}` / `pdfium.dll` from this directory
via `pdfium-render`'s `bind_to_library`. Source: bblanchon/pdfium-binaries
(Apache-2.0). macOS prod: the bundled dylib is signed/notarized with the app (same
Team ID → passes library validation, no special entitlement).
