// Take: the Offline Models page and Fine-tune on a downloaded model, from
// the header menu to the saved setup. One continuous recording, measurement
// included (cut in the edit), plus stills at each beat.
// YOAI_CAPTURE_MODEL picks the model (file name); default: the largest
// downloaded model with a Fine-tune button.
import { Take } from "./_capture";

const row = (m: string) => `[data-testid="downloaded-model"][data-model="${m}"]`;

describe("capture", () => {
  it("fine-tune", async () => {
    await browser.url("tauri://localhost/chat/");
    await (await $('[data-testid="chat-input"]')).waitForDisplayed({ timeout: 60_000 });

    const take = await Take.start("finetune");
    await take.hold(2000);

    // Into the Offline Models page the way a person goes: the menu.
    await take.click('[data-testid="header-menu"]');
    await take.hold(700);
    await take.click('[data-testid="menu-offline-models"]');
    await (await $('[data-testid="downloaded-models"]')).waitForDisplayed({ timeout: 60_000 });
    // Rows first, then their grades from the fit probe.
    await browser.waitUntil(
      async () => (await $$('[data-testid="downloaded-model"] [data-testid="model-fit"]').length) === (await $$('[data-testid="downloaded-model"]').length),
      { timeout: 60_000 },
    );
    await take.hold(2000);
    await take.still("01-offline-models");

    let model = process.env.YOAI_CAPTURE_MODEL ?? "";
    if (!model) {
      model = await browser.execute(() =>
        [...document.querySelectorAll('[data-testid="downloaded-model"]')]
          .filter((r) => r.querySelector('[data-testid="model-finetune"]'))
          .map((r) => {
            const m = (r.querySelector('[data-testid="model-detail"]')?.textContent ?? "").match(/([\d.]+)\s*GB/);
            return { name: (r as HTMLElement).dataset.model!, gb: m ? Number(m[1]) : 0 };
          })
          .sort((a, b) => b.gb - a.gb)[0]?.name ?? "",
      );
    }
    if (!model) throw new Error("capture: no downloaded model to fine-tune");
    console.log(`capture: model ${model}`);

    // The row: its grade (the tooltip says what it means), then Fine-tune.
    await take.scrollTo(row(model));
    await take.hover(`${row(model)} [data-testid="model-fit"]`, 2500);
    await take.still("02-fit-grade");
    await take.click(`${row(model)} [data-testid="model-finetune"]`);
    await (await $('[data-testid="tune-dialog"]')).waitForDisplayed({ timeout: 10_000 });
    await take.hold(2500);
    await take.still("03-tune-open");

    // The measurement, recorded whole.
    await take.hover('[data-testid="tune-measure"]', 800);
    await take.click('[data-testid="tune-measure"]');
    await (await $('[data-testid="tune-progress"]')).waitForDisplayed({ timeout: 30_000 });
    await take.hold(3000);
    await take.still("04-measuring");
    await (await $('[data-testid="tune-slider"]')).waitForDisplayed({ timeout: 1_200_000 });
    await take.hold(2500);
    await take.still("05-measured");

    // The trade-off: slide to the other end and let the card say what changes.
    const slider = await $('[data-testid="tune-slider"]');
    const max = Number(await slider.getAttribute("max"));
    const at = Number(await slider.getValue());
    if (max > 0) {
      await take.dragRange('[data-testid="tune-slider"]', at >= max / 2 ? 0 : max);
      await take.hold(3000);
      await take.still("06-picked");
    }

    await take.click('[data-testid="tune-manual"]');
    await take.hold(2500);
    await take.still("07-set-it-yourself");

    await take.click('[data-testid="tune-save"]');
    await (await $('[data-testid="tune-dialog"]')).waitForExist({ reverse: true, timeout: 120_000 });
    await take.hold(1500);
    await take.still("08-saved");
    await take.hover(`${row(model)} [data-testid="model-detail"]`, 3000);
    await take.still("09-row-fine-tuned");

    await take.stop();
  });
});
