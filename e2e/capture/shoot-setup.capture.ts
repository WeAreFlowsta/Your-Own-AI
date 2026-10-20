// Not a take: sets up a kept shoot profile once (YOAI_E2E_KEEP=1, YOAI_E2E_HOME=<the shoot's folder>, capture-fresh
// mode so the welcome shows). The welcome as in welcome.capture.ts (the Work set, the first AI renamed on a new
// picture), then step 3's "Sign in with Flowsta": the Vault asks "Connect Your Own AI?" and a person clicks Approve
// there. Links the identity the Vault has open. Prints the session (display name, agent key) at the end.
//   YOAI_CAPTURE_DRY=1 node e2e/run.mjs capture-fresh --spec e2e/capture/shoot-setup.capture.ts
import { Take } from "./_capture";

const card = '[data-testid="welcome-ai-card"]:first-of-type';
const gallery = "div.fixed.inset-0 div.grid";

describe("capture", () => {
  it("shoot setup", async () => {
    await (await $('[data-testid="welcome-title"]')).waitForDisplayed({ timeout: 60_000 });
    await browser.waitUntil(async () => (await $('[data-testid="welcome-recommended"]').getAttribute("data-pending")) === "0", { timeout: 60_000 });
    const take = await Take.start("shoot-setup");
    await take.click('[data-testid="welcome-download"]');
    await browser.waitUntil(async () => (await $$('[data-testid="welcome-ai-card"]')).length === 3, { timeout: 60_000 });
    await take.click('[data-testid="welcome-preset-option"]:nth-of-type(2)');
    await browser.waitUntil(async () => (await $(`${card} input`).getValue()) === "Assistant", { timeout: 30_000 });
    await take.click(`${card} button[title="Change picture"]`);
    await (await $(gallery)).waitForDisplayed({ timeout: 10_000 });
    await take.click(`${gallery} > button[title="${process.env.YOAI_CAPTURE_AI_PICTURE ?? "Lagoon"}"]`);
    await (await $(gallery)).waitForExist({ reverse: true, timeout: 10_000 });
    await take.type(`${card} input`, process.env.YOAI_CAPTURE_AI_NAME ?? "Nova", 20);
    await take.hold(1500);
    await (await $('//button[contains(., "Looks good")]')).click();

    // Step 3: connect the Vault's open identity.
    const flowsta = await $('[data-testid="wizard-flowsta"]');
    await flowsta.waitForDisplayed({ timeout: 30_000 });
    const signIn = await $('[data-testid="wizard-flowsta"] button[aria-label="Sign in with Flowsta"]');
    await signIn.waitForEnabled({ timeout: 60_000 });
    await signIn.click();
    console.log("shoot-setup: APPROVE in the Vault now (Connect Your Own AI?)");
    await browser.waitUntil(async () => (await flowsta.getText()).includes("Signed in with Flowsta"), {
      timeout: 600_000,
      interval: 2_000,
      timeoutMsg: "never signed in: was the Vault's Approve clicked?",
    });
    const session = await browser.execute(async () => (window as any).__TAURI__.core.invoke("flowsta_session"));
    console.log("shoot-setup: session " + JSON.stringify(session));
    await take.stop();
  });
});
