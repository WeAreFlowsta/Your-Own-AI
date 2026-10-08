// Flow 1: a fresh profile launches to the
// welcome screen and the hardware check names a model. The smallest proof
// that the window, the webview, the front end and the Rust side all came
// up - the test every release must pass first.
import { resolve } from "node:path";
import { SHOTS } from "../../wdio.conf";

const shot = (name: string) => browser.saveScreenshot(resolve(SHOTS, `${name}.png`));

describe("launch", () => {
  it("a fresh profile lands on the welcome screen", async () => {
    const title = await $('[data-testid="welcome-title"]');
    await title.waitForDisplayed({ timeout: 60_000 });
    await shot("01-welcome");
    expect((await title.getText()).length).toBeGreaterThan(0);
  });

  it("the hardware check recommends a model and offers the download", async () => {
    const rec = await $('[data-testid="welcome-recommended"]');
    // The Rust side measures the machine first ("One moment..") - wait for
    // the real label, which proves an invoke round trip.
    await browser.waitUntil(async () => (await rec.getAttribute("data-pending")) === "0", {
      timeout: 60_000,
      timeoutMsg: "the recommendation never resolved",
    });
    const label = await rec.getText();
    expect(label.length).toBeGreaterThan(0);
    expect(label).not.toContain("One moment");
    const download = await $('[data-testid="welcome-download"]');
    await expect(download).toBeEnabled();
    await shot("02-recommendation");
    // The download itself is never started here: that is a multi-GB fetch
    // and the person's choice, never a test's.
  });
});

describe("a download in flight on the wizard", () => {
  it("shows in the wizard's own pill and never covers its buttons", async () => {
    // Pretend the first model is downloading (the marker the wizard restores
    // on mount) and feed it a progress event the way the engine would -
    // nothing is downloaded.
    await browser.execute(() => {
      localStorage.setItem("firstModelDownloading", JSON.stringify({ filename: "e2e-fake-model.gguf", label: "Test model" }));
    });
    await browser.url("tauri://localhost/welcome/");
    const title = await $('[data-testid="welcome-title"]');
    await title.waitForDisplayed({ timeout: 60_000 });
    await browser.execute(async () => {
      const t = (window as any).__TAURI__;
      await t.event.emit("model-download-progress", { filename: "e2e-fake-model.gguf", downloaded: 1_100_000_000, total: 2_600_000_000, percent: 42 });
    });
    const pill = await $('[data-testid="welcome-progress"]');
    await pill.waitForDisplayed({ timeout: 10_000 });
    await browser.waitUntil(async () => (await pill.getText()).includes("42%"), { timeout: 10_000, timeoutMsg: "the wizard pill never showed the progress" });
    await shot("03-wizard-download-in-flight");
    // The activity tray must not be on this page at all.
    expect(await $('[data-testid="activity-tray"]').isExisting()).toBe(false);
    // And the primary button is the thing under its own centre.
    const covered = await browser.execute(() => {
      const b = document.querySelector('[data-testid="welcome-download"]') as HTMLElement;
      const r = b.getBoundingClientRect();
      const hit = document.elementFromPoint(r.left + r.width / 2, r.top + r.height / 2);
      return { onTop: !!hit && b.contains(hit), hit: hit ? (hit as HTMLElement).tagName + "." + (hit as HTMLElement).className.toString().slice(0, 40) : null };
    });
    expect(covered.onTop).toBe(true);
    await browser.execute(() => localStorage.removeItem("firstModelDownloading"));
  });
});

describe("step 2: meet your AIs", () => {
  it("shows the three AIs to edit after Download and continue", async () => {
    // Downloads are switched off for test launches (YOAI_BLOCK_MODEL_DOWNLOADS
    // in launch-app.sh), so the click moves to step 2 and fetches nothing.
    const download = await $('[data-testid="welcome-download"]');
    await download.waitForEnabled({ timeout: 60_000 });
    await download.click();
    await browser.waitUntil(async () => (await $$('[data-testid="welcome-ai-card"]')).length === 3, {
      timeout: 60_000,
      timeoutMsg: "the three AI cards never appeared on step 2",
    });
    await shot("04-meet-your-ais");
    const names = await browser.execute(() =>
      [...document.querySelectorAll('[data-testid="welcome-ai-card"] input')].map((i) => (i as HTMLInputElement).value),
    );
    expect(names.length).toBe(3);
    for (const n of names) expect(n.length).toBeGreaterThan(0);
  });

  it("offers Personal or Work as two choice cards and swaps the AIs on pick", async () => {
    const cards = () =>
      browser.execute(() =>
        [...document.querySelectorAll('[data-testid="welcome-ai-card"]')].map((c) => ({
          id: c.getAttribute("data-ai-id") ?? "",
          name: (c.querySelector("input") as HTMLInputElement).value,
          src: (c.querySelector("img") as HTMLImageElement).src,
        })),
      );
    // Capture the app's console for the report: a lost edit warns there.
    await browser.execute(() => {
      const w = window as any;
      w.__e2eLog = [];
      for (const k of ["warn", "error"] as const) {
        const orig = console[k];
        console[k] = (...a: unknown[]) => { w.__e2eLog.push(`${k}: ${a.map(String).join(" ")}`); orig(...a); };
      }
    });
    const choices = await $$('[data-testid="welcome-preset-option"]');
    expect(choices.length).toBe(2);
    expect(await choices[0].getAttribute("aria-pressed")).toBe("true");
    const before = await cards();
    await choices[1].click();
    // Every card keeps its AI, gets its Work name and a new picture.
    let after = before;
    try {
      await browser.waitUntil(
        async () => {
          after = await cards();
          return after.every((c, i) => c.id === before[i].id && c.name !== before[i].name && c.src !== before[i].src);
        },
        { timeout: 30_000 },
      );
    } finally {
      const log = await browser.execute(() => (window as any).__e2eLog as string[]);
      console.log("[preset] before", JSON.stringify(before.map((c) => [c.id.slice(0, 8), c.name])));
      console.log("[preset] after ", JSON.stringify(after.map((c) => [c.id.slice(0, 8), c.name, c.src.slice(0, 40)])));
      console.log("[preset] app console:", JSON.stringify(log));
    }
    expect(after.map((c) => c.name)).toEqual(["Assistant", "Coder", "Analyst"]);
    expect(await choices[1].getAttribute("aria-pressed")).toBe("true");
    await shot("04b-work-set");
    // Back to Personal so the personality spec sees the characters.
    await choices[0].click();
    await browser.waitUntil(
      async () => {
        const now = await cards();
        return now.every((c, i) => c.id === before[i].id && c.name === before[i].name);
      },
      { timeout: 30_000, timeoutMsg: "picking Personal again never restored the three AIs" },
    );
  });
});

describe("step 2: changing a personality", () => {
  it("offers the personalities, each with a real description", async () => {
    const change = await $('[data-testid="welcome-personality-change"]');
    await change.waitForDisplayed({ timeout: 30_000 });
    await change.click();
    await browser.waitUntil(async () => (await $$('[data-testid="welcome-personality-option"]')).length >= 6, {
      timeout: 15_000,
      timeoutMsg: "the personality picker never showed its choices",
    });
    await shot("05-personality-picker");
    const options = await browser.execute(() =>
      [...document.querySelectorAll('[data-testid="welcome-personality-option"]')].map((b) => (b as HTMLElement).innerText.replace(/\s+/g, " ").trim()),
    );
    expect(options.length % 2).toBe(0); // a full grid, no hole
    for (const text of options) {
      expect(text.length).toBeGreaterThan(20);
      expect(text.toLowerCase()).not.toContain("no description");
    }
    // Picking one changes the card's personality line.
    const second = (await $$('[data-testid="welcome-personality-option"]'))[1];
    const label = (await second.$("span").getText()).trim();
    await second.click();
    await browser.waitUntil(async () => (await change.getText()).includes(label), { timeout: 15_000, timeoutMsg: `the card never showed personality ${label}` });
    await shot("06-personality-changed");
  });
});
