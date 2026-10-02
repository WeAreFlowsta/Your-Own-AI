// The Offline Models page and Fine-tune on a downloaded model.
// Fine-tune measures for real (the engine loads the model a few times) and
// writes only to the scratch profile (model-tuning.json, tune-profiles.json
// in app data); the models folder is read, never written.
// YOAI_E2E_TUNE_MODEL picks the model (default: the smallest chat model).
import { invoke, scrollTo, shot } from "./_helpers";

const row = (model: string) => `[data-testid="downloaded-model"][data-model="${model}"]`;

async function openModelsPage() {
  await browser.url("tauri://localhost/setup/");
  await (await $('[data-testid="downloaded-models"]')).waitForDisplayed({ timeout: 60_000 });
}

/** The row's detail line once the fit probe has filled it in ("runs at …"). */
async function rowDetail(model: string) {
  const d = await $(`${row(model)} [data-testid="model-detail"]`);
  await browser.waitUntil(async () => (await d.getText()).includes("runs at"), {
    timeout: 60_000,
    timeoutMsg: `no fit line for ${model}`,
  });
  return d.getText();
}

async function openTune(model: string) {
  await scrollTo(`${row(model)} [data-testid="model-finetune"]`);
  await $(`${row(model)} [data-testid="model-finetune"]`).click();
  await (await $('[data-testid="tune-dialog"]')).waitForDisplayed({ timeout: 10_000 });
  // The dialog reads the saved tuning and any measured table in a visible task.
  await browser.pause(500);
}

const source = () => $('[data-testid="tune-source"]').getAttribute("data-source");

async function setSlider(sel: string, v: number) {
  await browser.execute((s: string, val: number) => {
    const el = document.querySelector(s) as HTMLInputElement;
    el.value = String(val);
    el.dispatchEvent(new Event("input", { bubbles: true }));
  }, sel, v);
}

async function saveAndClose() {
  await $('[data-testid="tune-save"]').click();
  await (await $('[data-testid="tune-dialog"]')).waitForExist({ reverse: true, timeout: 120_000 });
}

describe("offline models", () => {
  let model = "";

  it("lists the downloaded models, each with a fit grade", async () => {
    await openModelsPage();
    const listed = await invoke<{ name: string }[]>("list_local_models");
    const rows = (await $$('[data-testid="downloaded-model"]').map((r) => r.getAttribute("data-model"))) as string[];
    expect(rows.length).toBeGreaterThan(0);
    // Every row is a file the engine sees (the list leaves out vision
    // projectors and the embedding model, which are not chat models).
    const names = listed.map((m) => m.name);
    for (const r of rows) expect(names).toContain(r);
    expect(rows.some((r) => /mmproj|bge-/i.test(r))).toBe(false);
    // The grades arrive with the fit probe, after the rows.
    await browser.waitUntil(async () => (await $$('[data-testid="downloaded-model"] [data-testid="model-fit"]').length) === rows.length, {
      timeout: 60_000,
      timeoutMsg: "not every downloaded model got a fit grade",
    });
    await shot("40-offline-models");

    model = process.env.YOAI_E2E_TUNE_MODEL ?? "";
    if (!model) {
      const sized = await browser.execute(() =>
        [...document.querySelectorAll('[data-testid="downloaded-model"]')]
          .filter((r) => r.querySelector('[data-testid="model-finetune"]'))
          .map((r) => {
            const t = r.querySelector('[data-testid="model-detail"]')?.textContent ?? "";
            const m = t.match(/([\d.]+)\s*GB/);
            return { name: (r as HTMLElement).dataset.model!, gb: m ? Number(m[1]) : Infinity };
          }),
      );
      model = sized.sort((a, b) => a.gb - b.gb)[0]?.name ?? "";
    }
    expect(model).not.toBe("");
    console.log(`e2e: fine-tune model: ${model}`);
  });

  it("filters the catalog by task", async () => {
    const tabs = await $$('[data-testid="catalog-tab"]');
    expect(tabs.length).toBeGreaterThan(1);
    await scrollTo('[data-testid="catalog-tab"]');
    for (const key of ["all", "coding", "vision"]) {
      const tab = await $(`[data-testid="catalog-tab"][data-key="${key}"]`);
      if (!(await tab.isExisting())) continue;
      await tab.click();
      await browser.waitUntil(async () => (await tab.getAttribute("aria-pressed")) === "true", { timeout: 5_000 });
      const count = Number((await tab.getText()).match(/(\d+)\s*$/)?.[1] ?? NaN);
      const cards = await $$('[data-testid="catalog-family"]').length;
      // Cards for models too big for this machine sit behind a "show" line.
      expect(cards).toBeLessThanOrEqual(count);
      if (count > 0) expect(cards).toBeGreaterThan(0);
      await shot(`41-catalog-${key}`);
    }
    await $('[data-testid="catalog-tab"][data-key="all"]').click();
  });

  // Measuring loads the model several times: well past the default 5 minutes.
  it("measures a model on this computer and keeps a picked setup", async () => {
    await openModelsPage();
    const before = await rowDetail(model);
    await openTune(model);
    expect(await source()).toBe("Automatic");
    await shot("42-tune-open");

    // The measurement: a few loads of the model, a progress line per arm.
    await $('[data-testid="tune-measure"]').click();
    await (await $('[data-testid="tune-progress"]')).waitForDisplayed({ timeout: 30_000 });
    await browser.pause(3_000);
    await shot("43-tune-measuring");
    await (await $('[data-testid="tune-slider"]')).waitForDisplayed({ timeout: 600_000, timeoutMsg: "the measurement never finished" });
    const note = await $('[data-testid="tune-note"]');
    if (await note.isExisting()) throw new Error(`measure failed: ${await note.getText()}`);
    const profile = await invoke<{ results: { ctx: number; gen_tps: number; failed?: string | null }[] } | null>("tune_profiles_get", { model });
    const timed = (profile?.results ?? []).filter((r) => !r.failed && r.gen_tps > 0);
    expect(timed.length).toBeGreaterThan(1);
    // The slider opens on what the model runs at now.
    expect(await source()).toBe("Automatic");
    await shot("44-tune-measured");

    // Pick the stop furthest from the automatic one: the card follows, the
    // chip says the setup was measured here.
    const slider = await $('[data-testid="tune-slider"]');
    const max = Number(await slider.getAttribute("max"));
    const at = Number(await slider.getValue());
    const target = at >= max / 2 ? 0 : max;
    await setSlider('[data-testid="tune-slider"]', target);
    const picked = Number(await $('[data-testid="tune-pick"]').getAttribute("data-ctx"));
    await browser.waitUntil(async () => (await source()) === "Measured here", { timeout: 5_000, timeoutMsg: `source stayed ${await source()}` });
    expect(await $('[data-testid="tune-current"]').getText()).toContain(`${Math.round(picked / 1024)}K context`);
    await shot("45-tune-picked");

    // Mark the page: if Save reloads it, the mark is gone afterwards.
    await browser.execute(() => { (window as any).__e2eMark = 1; });
    await saveAndClose();
    await browser.pause(500);
    const after = await browser.execute(() => ({ mark: (window as any).__e2eMark ?? 0, path: location.pathname, rows: document.querySelectorAll('[data-testid="downloaded-model"]').length }));
    console.log(`e2e: after Save: ${JSON.stringify(after)}`);
    await shot("46-tune-saved");
    expect(after.mark).toBe(1);
    const kept = await invoke<{ context?: number }>("tuning_get", { model });
    expect(kept.context).toBe(picked);

    // The row says so at once, and again on a fresh page.
    await browser.waitUntil(async () => (await rowDetail(model)).includes("fine-tuned"), {
      timeout: 30_000,
      timeoutMsg: "the row did not say fine-tuned after Save",
    });
    await openModelsPage();
    await browser.waitUntil(async () => (await rowDetail(model)).includes("fine-tuned"), {
      timeout: 30_000,
      timeoutMsg: `the row never said fine-tuned (was: ${before})`,
    });
    await scrollTo(row(model));
    await shot("47-row-fine-tuned");
  }).timeout(900_000);

  it("goes back to automatic", async () => {
    await openTune(model);
    // A setup set by hand opens the drawer and the measured table again.
    expect(await source()).toBe("Measured here");
    await (await $('[data-testid="tune-slider"]')).waitForDisplayed({ timeout: 5_000 });
    await (await $('[data-testid="tune-back-to-auto"]')).waitForDisplayed({ timeout: 5_000 });
    await shot("48-tune-reopened");
    await $('[data-testid="tune-back-to-auto"]').click();
    await browser.waitUntil(async () => (await source()) === "Automatic", { timeout: 5_000 });
    await shot("49-tune-automatic");
    await saveAndClose();
    await browser.waitUntil(async () => !(await rowDetail(model)).includes("fine-tuned"), {
      timeout: 30_000,
      timeoutMsg: "the row still said fine-tuned after going back to automatic",
    });
    const kept = await invoke<Record<string, unknown>>("tuning_get", { model });
    expect(Object.keys(kept).filter((k) => kept[k] != null && kept[k] !== false)).toEqual([]);
    await openModelsPage();
    expect(await rowDetail(model)).not.toContain("fine-tuned");
  });
});
