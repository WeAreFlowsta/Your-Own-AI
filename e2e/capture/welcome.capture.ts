// Take: the first minute on a fresh profile - the model picked for this computer (step 1), then the AIs (step 2):
// the Work set instead of Personal, and the first Work AI given a new picture and name, the AI the release video
// follows from then on. Stops on step 2: it never reaches step 3 (Connect with Flowsta), so a retake leaves nothing
// behind in any identity.
//   YOAI_CAPTURE_AI_NAME     the AI's new name (default Nova)
//   YOAI_CAPTURE_AI_PICTURE  the picture's name in the gallery (its title); default Lagoon (the teal gradient)
import { Take } from "./_capture";

const card = '[data-testid="welcome-ai-card"]:first-of-type';
const gallery = "div.fixed.inset-0 div.grid";

describe("capture", () => {
  it("welcome", async () => {
    await (await $('[data-testid="welcome-title"]')).waitForDisplayed({ timeout: 60_000 });
    await browser.waitUntil(async () => (await $('[data-testid="welcome-recommended"]').getAttribute("data-pending")) === "0", {
      timeout: 60_000,
      timeoutMsg: "the hardware check never named a model",
    });
    console.log(`capture: recommended ${await $('[data-testid="welcome-recommended"]').getText()}`);

    const take = await Take.start("welcome");
    await take.hold(1200);
    await take.still("01-step1-model");
    // The model picked for this computer: rest on it, then on.
    await take.hover('[data-testid="welcome-recommended"]', 1800);
    await take.click('[data-testid="welcome-download"]');
    await browser.waitUntil(async () => (await $$('[data-testid="welcome-ai-card"]')).length === 3, {
      timeout: 60_000,
      timeoutMsg: "step 2 never showed the three AIs",
    });
    await take.hold(1400);
    await take.still("02-step2-personal");

    // The Work set.
    await take.click('[data-testid="welcome-preset-option"]:nth-of-type(2)');
    await browser.waitUntil(async () => (await $(`${card} input`).getValue()) === "Assistant", {
      timeout: 30_000,
      timeoutMsg: "the Work set never arrived",
    });
    await take.hold(1300);
    await take.still("03-step2-work");

    // Its first AI: a new picture.
    await take.click(`${card} button[title="Change picture"]`);
    await (await $(gallery)).waitForDisplayed({ timeout: 10_000 });
    await take.hold(900);
    await take.still("03b-gallery");
    const pic = process.env.YOAI_CAPTURE_AI_PICTURE ?? "Lagoon";
    const choice = `${gallery} > button[title="${pic}"]`;
    await take.hover(choice, 500);
    await take.click(choice);
    await (await $(gallery)).waitForExist({ reverse: true, timeout: 10_000 });
    await take.hold(900);

    // And a new name.
    await take.type(`${card} input`, process.env.YOAI_CAPTURE_AI_NAME ?? "Nova");
    await take.hold(1500);
    await take.still("04-step2-renamed");
    await take.hold(1200);
    await take.stop();
  });
});
