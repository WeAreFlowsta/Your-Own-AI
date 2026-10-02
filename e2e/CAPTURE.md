# Video captures: the real app, recorded

A capture is a scripted take of one flow in the real app. The real mouse
pointer moves and clicks, so hover states, tooltips and the cursor are in
the recording. The take records the window's client area at 1920x1080 and
saves full-size stills at each beat. The same script re-shoots the flow
after every release at the same size and pace.

```
npm run capture                     # every take in e2e/capture/
node e2e/run.mjs capture --spec e2e/capture/finetune.capture.ts
```

Each take writes `e2e/captures/<take>-<time>/`:

| File | What |
|---|---|
| `<take>.mp4` | the recording, 60 fps, near-lossless (NVENC when the GPU has it, else x264) |
| `stills/NN-*.png` | the capture area at each beat, full size, no cursor |
| `clicks.jsonl` | one line per move, click, drag, scroll and still: seconds from the recording's start, the element, its position in the frame - a guide for zooms in the edit |

A capture runs in the same scratch profile as the UI tests (`e2e/profile`),
reading the machine's real models folder and its installed CUDA engine.
Nothing in the person's own profile is touched; a take writes only to
the scratch profile.

Takes:

- `finetune.capture.ts` - the header menu to Offline Models, a model's fit
  grade (tooltip), Fine-tune, the whole measurement, the slider moved to
  the other end, "Set it yourself", Save, the row saying "fine-tuned".
  `YOAI_CAPTURE_MODEL=<file name>` picks the model; the default is the
  largest downloaded model.

Settings (environment variables):

| Variable | Effect |
|---|---|
| `YOAI_CAPTURE_MODEL` | the model a take uses (file name, e.g. `LFM2.5-8B-A1B-Q4_K_M.gguf`) |
| `YOAI_CAPTURE_WIDTH` / `YOAI_CAPTURE_HEIGHT` | the client area, physical pixels (default 1920 x 1080) |
| `YOAI_CAPTURE_ENCODER` | `nvenc` or `x264` (default: NVENC when ffmpeg lists it) |
| `YOAI_CAPTURE_DRY` | no recording, no real pointer: driver clicks and WebDriver screenshots, to check a take's steps on a machine that cannot record |
| `YOAI_E2E_MODELS_DIR` / `YOAI_E2E_ENGINES_DIR` | override where the real models / engines are read from |

## Setting up a Windows capture machine

Done once. Every step is a command to run in PowerShell unless it says
otherwise; check each step's result before the next.

### 1. Tools

```powershell
winget install --id Git.Git -e
winget install --id OpenJS.NodeJS.LTS -e
winget install --id Rustlang.Rustup -e
winget install --id GitHub.cli -e
winget install --id Gyan.FFmpeg -e
winget install --id Microsoft.VisualStudio.2022.BuildTools -e --override "--quiet --wait --add Microsoft.VisualStudio.Workload.VCTools --includeRecommended"
```

Open a NEW terminal afterwards (PATH changes), then check:

```powershell
git --version; node --version; cargo --version; gh --version; ffmpeg -hide_banner -encoders | Select-String nvenc
```

`node` must be 20 or newer. The ffmpeg line should list `h264_nvenc` on an
NVIDIA card. WebView2 ships with Windows 11. `gh auth login` once (the
engine download below uses it).

### 2. The repository and its downloaded parts

```powershell
git clone https://github.com/WeAreFlowsta/Your-Own-AI.git
cd Your-Own-AI
npm ci
```

The bundled binaries are not in git; fetch the same ones the release build
uses (the versions are in `.github/workflows/build-release.yml`: the llama
engine `RELEASE_TAG` and `HC_RELEASE`; update these lines if they moved):

```powershell
gh release download llama-b10809 --repo WeAreFlowsta/Your-Own-AI --pattern "llama-server-x86_64-pc-windows-msvc.exe" --dir src-tauri/bin
New-Item -ItemType Directory -Force src-tauri/binaries | Out-Null
foreach ($b in "holochain", "lair-keystore") {
  Invoke-WebRequest "https://github.com/holochain/holochain/releases/download/holochain-0.6.1/$b-x86_64-pc-windows-msvc.exe" -OutFile "src-tauri/binaries/yourowai-$b-x86_64-pc-windows-msvc.exe"
}
```

pdfium (scanned-PDF reading) from Git Bash, which Git for Windows installed:

```bash
./scripts/fetch-pdfium.sh win-x64
```

### 3. The test build

```powershell
npm run e2e:build
```

The first build compiles everything (~15-25 minutes) and leaves a debug
build of a few GB in `src-tauri\target\debug`. It carries the embedded
WebDriver server, which no release build has.

### 4. The real app, set up for the take

The scratch profile READS the installed app's models and engine:

- Install Your Own AI (the normal installer), download the models the
  video should show, and install the CUDA engine in the app (Settings).
- **Quit the app before every run** (tray icon -> Quit): it is single
  instance, and the test build will not start beside it.

### 5. The display

The take records a 1920x1080 client area, so the screen must be bigger than
that plus the window's title bar: a 1440p or 4K monitor (a 1080p screen
cannot fit it; the take stops and says so).

Windows' display scale decides how the app LOOKS at that size:

| Monitor / scale | App layout | Looks like |
|---|---|---|
| 2560x1440 at 100% | 1920 x 1080 | a big window, small text |
| 2560x1440 or 3840x2160 at 150% | 1280 x 720 | larger, readable UI, rendered sharp at 1080p |

150% is the better default for videos. Set it before the run (Settings ->
System -> Display -> Scale).

While a take records:

- Turn on Do not disturb (no notification toasts in the frame).
- Set the screen to never turn off (Settings -> System -> Power), or
  the recording freezes.
- Leave the mouse alone: the take moves it.

### 6. First run: prove the harness, then take

The UI tests have not yet run on Windows. Run one spec first:

```powershell
node e2e/run.mjs models --spec e2e/specs/models/offline-models.e2e.ts
```

Screenshots land in `e2e\shots`. Then the take:

```powershell
$env:YOAI_CAPTURE_MODEL = "<model file name>"
node e2e/run.mjs capture --spec e2e/capture/finetune.capture.ts
```

Afterwards, check Task Manager for a leftover `llama-server` from
`src-tauri\bin` (the sidecar cleanup between sessions runs on Linux and
macOS only so far) and end it.

### If something fails

- **The app never starts / the service cannot launch the `.cmd`:**
  `e2e/launch-app.cmd` has not run on Windows before. Check the WebdriverIO
  log; the service may need `appBinaryPath` pointed at
  `src-tauri\target\debug\app.exe` with the scratch-profile variables set
  another way.
- **Clicks land in the wrong place:** the pointer helper
  (`e2e/capture/pointer-win.ps1`) works in physical pixels and the page
  positions are CSS pixels x `devicePixelRatio`. If a run's moves are off
  by the scale factor, check that the helper started per-monitor DPI aware.
- **"the client area is NxM, wanted 1920x1080":** the screen is too small
  for the window at that size, or the window was snapped or maximized.
- **Black or frozen video:** the screen turned off or locked during the
  take.
