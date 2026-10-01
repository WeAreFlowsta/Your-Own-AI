# UI tests: the real app, driven

These tests drive the real Your Own AI window through an embedded WebDriver
server (`tauri-plugin-wdio-webdriver`, compiled in only with the `e2e`
cargo feature; no release build carries it - `scripts/check-no-e2e-feature.mjs`
proves that on every build).

## On any machine

What the app itself needs to build (Rust, the Tauri system packages, Node),
then:

```bash
npm ci                 # WebdriverIO and the Tauri service come as dev deps
npm run e2e:build      # tauri build --debug --no-bundle --features e2e (~5 min, ~700 MB)
npm run e2e            # fresh profile: the welcome flow
npm run e2e:models     # the machine's installed models: the chat flows
```

Screenshots land in `e2e/shots/` (one per step, plus one per test marked
`ok_` or `FAIL_`). The app runs in a scratch profile under `e2e/profile/`
(wiped per run); the person's own profile is never touched. Close the real
app first: it is single-instance.

Per platform:

- **Linux desktop (Wayland or X11):** `sudo apt install xdotool`. The window
  must be ACTIVE or the compositor withholds frame callbacks and the webview
  produces no frames (Qwik's visible tasks never run); `wdio.conf.ts`
  activates it with xdotool. Not Xvfb on NVIDIA: WebKitGTK segfaults without
  DRI3. Mesa-based CI runners are untested.
- **Windows:** `e2e/launch-app.cmd` is the launcher (written, not yet run);
  the service uses the embedded driver, so no Edge WebDriver is needed.
- **macOS:** the embedded driver is auto-selected; untested so far.

## Launch scenarios (environment variables, see `launch-app.sh`)

| Variable | Effect |
|---|---|
| `YOAI_E2E_WITH_MODELS=1` | the scratch profile reads the machine's real models folder (`YOAI_E2E_MODELS_DIR` overrides the path); specs must never delete or download |
| `YOAI_E2E_CPU_ONLY=1` | the app's `FLOWSTA_CPU_ONLY` switch: every layer on the CPU |
| `YOAI_E2E_WITH_CUDA=1` | the machine's installed CUDA engine folder linked in (`YOAI_E2E_ENGINES_DIR` overrides) |
| `YOAI_E2E_NATIVE_WAYLAND=1` | keep the native Wayland window (default on Wayland is an XWayland window, which recorders can see) |

Identity and online scenarios are planned, not built: a dedicated test
identity, a seeded session, a staging proxy.

## Writing a spec

- Select by `data-testid`, never by copy. The metal button takes `testId`.
- The composer is a contenteditable: type with `typeInto` (set the text and
  dispatch an `InputEvent`); the driver's key events do not reach it.
- A reply is read from `[data-testid="chat-message"][data-role="assistant"]`
  once its `data-state` is `done`, never from the page text.
- `console.log` is stripped from built front ends: diagnose with
  `browser.execute`, and look at the screenshots.
- No downloads, no deletions, nothing that leaves the scratch profile.
