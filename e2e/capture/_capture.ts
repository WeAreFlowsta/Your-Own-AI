// Video captures of the real app: the real pointer, a screen recording of
// the window's client area at a fixed size, and full-size stills at the
// moments that matter (see e2e/CAPTURE.md).
//
// The pointer is the OS pointer, moved and clicked for real, so hover
// styles, tooltips and the cursor itself are in the recording. The driver
// is used only to READ the page (where an element is, whether a step has
// finished), never to click.
import { execFileSync, spawn, type ChildProcess } from "node:child_process";
import { appendFileSync, mkdirSync } from "node:fs";
import { resolve } from "node:path";
import { createInterface } from "node:readline";

const here = resolve(import.meta.dirname ?? ".");
const WIN = process.platform === "win32";
const TITLE = "Your Own AI";
/** The recorded client area, physical pixels. 1920x1080 so a take drops
 *  into a 1080p edit as is, with room to zoom. */
const W = Number(process.env.YOAI_CAPTURE_WIDTH ?? 1920);
const H = Number(process.env.YOAI_CAPTURE_HEIGHT ?? 1080);
/** No recording and no real pointer: the flow runs with driver clicks and
 *  WebDriver screenshots, to check a take's steps on a machine that cannot
 *  record (a Wayland desktop). */
const DRY = !!process.env.YOAI_CAPTURE_DRY;

type Rect = { x: number; y: number; w: number; h: number };

// ---------------------------------------------------------------- pointer

interface Pointer {
  size(x: number, y: number, w: number, h: number): Promise<void>;
  rect(): Promise<Rect>;
  screen(): Promise<{ w: number; h: number }>;
  glide(x: number, y: number, ms: number): Promise<void>;
  down(): Promise<void>;
  up(): Promise<void>;
  wheel(n: number): Promise<void>;
  close(): void;
}

/** Windows: one long-lived PowerShell answering line by line (pointer-win.ps1). */
async function windowsPointer(): Promise<Pointer> {
  const ps = spawn("powershell.exe", ["-NoProfile", "-ExecutionPolicy", "Bypass", "-File", resolve(here, "pointer-win.ps1")], {
    stdio: ["pipe", "pipe", "inherit"],
  });
  const lines = createInterface({ input: ps.stdout! });
  const waiting: ((s: string) => void)[] = [];
  lines.on("line", (l) => waiting.shift()?.(l));
  const ask = (cmd: string) =>
    new Promise<string>((ok, fail) => {
      waiting.push((l) => (l.startsWith("err") ? fail(new Error(`pointer: ${cmd}: ${l}`)) : ok(l)));
      ps.stdin!.write(cmd + "\n");
    });
  // The window can take a moment to map after the session starts.
  for (let i = 0; ; i++) {
    try { await ask(`find ${TITLE}`); break; } catch (e) { if (i > 30) throw e; await new Promise((r) => setTimeout(r, 1000)); }
  }
  const nums = (s: string) => s.split(" ").map(Number);
  return {
    size: async (x, y, w, h) => { await ask(`size ${x} ${y} ${w} ${h}`); },
    rect: async () => { const [x, y, w, h] = nums(await ask("rect")); return { x, y, w, h }; },
    screen: async () => { const [w, h] = nums(await ask("screen")); return { w, h }; },
    glide: async (x, y, ms) => { await ask(`glide ${Math.round(x)} ${Math.round(y)} ${Math.round(ms)}`); },
    down: async () => { await ask("down"); },
    up: async () => { await ask("up"); },
    wheel: async (n) => { await ask(`wheel ${n}`); },
    close: () => ps.kill(),
  };
}

/** Linux (X11, or the XWayland window launch-app.sh makes): xdotool. */
async function x11Pointer(): Promise<Pointer> {
  const xdo = (...a: string[]) => execFileSync("xdotool", a, { encoding: "utf8", timeout: 10_000 }).trim();
  let id = "";
  for (let i = 0; !id; i++) {
    try { id = xdo("search", "--onlyvisible", "--name", `^${TITLE}$`).split("\n").pop() ?? ""; } catch { /* not yet */ }
    if (!id) { if (i > 30) throw new Error("pointer: no app window"); await new Promise((r) => setTimeout(r, 1000)); }
  }
  let at = { x: 0, y: 0 };
  return {
    size: async (x, y, w, h) => { xdo("windowsize", id, String(w), String(h)); xdo("windowmove", id, String(x), String(y)); xdo("windowactivate", "--sync", id); },
    rect: async () => {
      const g = Object.fromEntries(xdo("getwindowgeometry", "--shell", id).split("\n").map((l) => l.split("=")));
      return { x: Number(g.X), y: Number(g.Y), w: Number(g.WIDTH), h: Number(g.HEIGHT) };
    },
    screen: async () => { const [w, h] = xdo("getdisplaygeometry").split(" ").map(Number); return { w, h }; },
    glide: async (x, y, ms) => {
      const from = at, t0 = Date.now();
      for (;;) {
        const t = Math.min(1, (Date.now() - t0) / Math.max(1, ms));
        const e = t < 0.5 ? 4 * t * t * t : 1 - Math.pow(-2 * t + 2, 3) / 2;
        xdo("mousemove", String(Math.round(from.x + (x - from.x) * e)), String(Math.round(from.y + (y - from.y) * e)));
        if (t >= 1) break;
        await new Promise((r) => setTimeout(r, 8));
      }
      at = { x, y };
    },
    down: async () => { xdo("mousedown", "1"); },
    up: async () => { xdo("mouseup", "1"); },
    wheel: async (n) => { for (let i = 0; i < Math.abs(n); i++) xdo("click", n > 0 ? "4" : "5"); },
    close: () => {},
  };
}

// --------------------------------------------------------------- recorder

function ffmpegInput(r: Rect, mouse: boolean): string[] {
  return WIN
    ? ["-f", "gdigrab", "-framerate", "60", "-offset_x", String(r.x), "-offset_y", String(r.y), "-video_size", `${r.w}x${r.h}`, "-draw_mouse", mouse ? "1" : "0", "-i", "desktop"]
    : ["-f", "x11grab", "-framerate", "60", "-video_size", `${r.w}x${r.h}`, "-draw_mouse", mouse ? "1" : "0", "-i", `${process.env.DISPLAY ?? ":0"}+${r.x},${r.y}`];
}

/** NVENC when this ffmpeg has it (the 5080 box), else x264. Near-lossless
 *  either way: the edit compresses, the take should not. */
function encoderArgs(): string[] {
  const pick = process.env.YOAI_CAPTURE_ENCODER;
  let nvenc = pick === "nvenc";
  if (!pick) {
    try { nvenc = execFileSync("ffmpeg", ["-hide_banner", "-encoders"], { encoding: "utf8" }).includes("h264_nvenc"); } catch { /* x264 */ }
  }
  return nvenc
    ? ["-c:v", "h264_nvenc", "-preset", "p5", "-tune", "hq", "-rc", "vbr", "-cq", "16", "-b:v", "0", "-pix_fmt", "yuv420p"]
    : ["-c:v", "libx264", "-preset", "veryfast", "-crf", "16", "-pix_fmt", "yuv420p"];
}

// -------------------------------------------------------------------- take

export class Take {
  private constructor(
    private dir: string,
    private pointer: Pointer | null,
    private area: Rect,
    private rec: ChildProcess | null,
    private t0: number,
  ) {}

  /** Sizes the window to the capture area, then starts recording. */
  static async start(flow: string): Promise<Take> {
    const stamp = new Date().toISOString().replace(/[:.]/g, "-").slice(0, 19);
    const dir = resolve(here, "..", "captures", `${flow}-${stamp}`);
    mkdirSync(resolve(dir, "stills"), { recursive: true });
    if (DRY) {
      console.log(`capture: DRY run (no recording, driver clicks) -> ${dir}`);
      return new Take(dir, null, { x: 0, y: 0, w: W, h: H }, null, Date.now());
    }
    const pointer = WIN ? await windowsPointer() : await x11Pointer();
    const screen = await pointer.screen();
    await pointer.size(0, 0, W, H);
    await new Promise((r) => setTimeout(r, 1500)); // the webview relays out
    const area = await pointer.rect();
    if (area.w !== W || area.h !== H) {
      throw new Error(`capture: the window's client area is ${area.w}x${area.h}, wanted ${W}x${H} - the screen (${screen.w}x${screen.h}) may be too small; see e2e/CAPTURE.md`);
    }
    if (area.x + area.w > screen.w || area.y + area.h > screen.h) {
      throw new Error(`capture: the ${W}x${H} client area at ${area.x},${area.y} runs off the ${screen.w}x${screen.h} screen; see e2e/CAPTURE.md`);
    }
    const out = resolve(dir, `${flow}.mp4`);
    const rec = spawn("ffmpeg", ["-hide_banner", "-loglevel", "error", "-y", ...ffmpegInput(area, true), ...encoderArgs(), out], { stdio: ["pipe", "inherit", "inherit"] });
    const t0 = Date.now();
    await new Promise((r) => setTimeout(r, 1000)); // first frames before the first move
    console.log(`capture: recording ${W}x${H} at ${area.x},${area.y} -> ${out}`);
    return new Take(dir, pointer, area, rec, t0);
  }

  private log(kind: string, sel: string, x?: number, y?: number) {
    // Times are from the recording's start (within a frame or two): the
    // edit's guide for where to zoom and when.
    appendFileSync(resolve(this.dir, "clicks.jsonl"), JSON.stringify({ t: (Date.now() - this.t0) / 1000, kind, sel, x, y }) + "\n");
  }

  /** The element's center in screen pixels (CSS rect x devicePixelRatio). */
  private async center(sel: string): Promise<{ x: number; y: number }> {
    const el = await $(sel);
    await el.waitForDisplayed({ timeout: 30_000 });
    const r = await browser.execute((s: string) => {
      const b = document.querySelector(s)!.getBoundingClientRect();
      return { x: b.left + b.width / 2, y: b.top + b.height / 2, dpr: window.devicePixelRatio };
    }, sel);
    return { x: this.area.x + r.x * r.dpr, y: this.area.y + r.y * r.dpr };
  }

  hold(ms: number) {
    return new Promise((r) => setTimeout(r, ms));
  }

  /** Brings an element into view the way a person scrolls: smoothly. */
  async scrollTo(sel: string) {
    await browser.execute((s: string) => document.querySelector(s)?.scrollIntoView({ block: "center", behavior: "smooth" }), sel);
    await this.hold(900);
    this.log("scroll", sel);
  }

  async moveTo(sel: string, ms?: number) {
    if (!this.pointer) return;
    const c = await this.center(sel);
    await this.pointer.glide(c.x, c.y, ms ?? 650);
    this.log("move", sel, c.x - this.area.x, c.y - this.area.y);
  }

  /** Rests on an element (hover styles, a tooltip after ~1 s). */
  async hover(sel: string, ms = 1500) {
    await this.moveTo(sel);
    await this.hold(ms);
  }

  async click(sel: string) {
    if (!this.pointer) {
      await $(sel).click();
      this.log("click", sel);
      return;
    }
    await this.moveTo(sel);
    await this.hold(150);
    await this.pointer.down();
    await this.hold(70);
    await this.pointer.up();
    this.log("click", sel);
    await this.hold(250);
  }

  /** Drags a range input's thumb to a step, with the real button held. */
  async dragRange(sel: string, to: number, ms = 1200) {
    const r = await browser.execute((s: string) => {
      const el = document.querySelector(s) as HTMLInputElement;
      const b = el.getBoundingClientRect();
      return { l: b.left, w: b.width, y: b.top + b.height / 2, min: Number(el.min), max: Number(el.max), v: Number(el.value), dpr: window.devicePixelRatio };
    }, sel);
    const thumb = 16; // the painted thumb's width, CSS px
    const at = (v: number) => this.area.x + (r.l + thumb / 2 + ((v - r.min) / Math.max(1, r.max - r.min)) * (r.w - thumb)) * r.dpr;
    const y = this.area.y + r.y * r.dpr;
    if (!this.pointer) {
      await browser.execute((s: string, v: number) => {
        const el = document.querySelector(s) as HTMLInputElement;
        el.value = String(v);
        el.dispatchEvent(new Event("input", { bubbles: true }));
      }, sel, to);
      this.log("drag", sel);
      return;
    }
    await this.pointer.glide(at(r.v), y, 600);
    await this.hold(200);
    await this.pointer.down();
    await this.pointer.glide(at(to), y, ms);
    await this.hold(120);
    await this.pointer.up();
    this.log("drag", sel, at(to) - this.area.x, y - this.area.y);
  }

  /** A full-size still of the capture area, without the cursor. */
  async still(name: string) {
    const file = resolve(this.dir, "stills", `${name}.png`);
    if (!this.pointer) {
      await browser.saveScreenshot(file);
    } else {
      execFileSync("ffmpeg", ["-hide_banner", "-loglevel", "error", "-y", ...ffmpegInput(this.area, false), "-frames:v", "1", file], { timeout: 20_000 });
    }
    this.log("still", name);
  }

  async stop() {
    await this.hold(1000);
    if (this.rec) {
      const done = new Promise((r) => this.rec!.on("exit", r));
      this.rec.stdin!.write("q"); // ffmpeg finishes the file cleanly on q
      await done;
    }
    this.pointer?.close();
    console.log(`capture: done -> ${this.dir}`);
  }
}
