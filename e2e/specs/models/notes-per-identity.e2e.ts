// The conversation notes (the continue pointer, renames, project folders)
// are kept per profile: the window's storage is one for every identity on a
// computer. A continue pointer an older build left for another identity is
// not offered here, and stays for its owner; one naming this profile's own
// AI is taken over.
import { waitForChat, invoke } from "./_helpers";

async function reloadToChat() {
  await browser.execute(() => location.reload());
  await browser.pause(2000);
  await waitForChat();
  // The notes are scoped once the profile is known (a few hundred ms).
  await browser.pause(3000);
}

describe("conversation notes per identity", () => {
  it("offers this profile's continue pointer only", async function () {
    this.timeout(300_000);
    await waitForChat();
    const path = await invoke<string>("profile_store_path", { name: "ai-data.json" });
    const scope = path.split(/[\\/]/).slice(-2, -1)[0];
    const own = await browser.execute(async (p: string) => {
      const t = (window as any).__TAURI__.core;
      const rid = await t.invoke("plugin:store|load", { path: p, autoSave: false });
      const got = await t.invoke("plugin:store|get", { rid, key: "custom-ais" });
      const list = Array.isArray(got) ? got[0] : got;
      return (Array.isArray(list) ? list : []).map((a: any) => a.agentPubKey ?? "").find((k: string) => !!k) ?? "";
    }, path);
    expect(own).toBeTruthy();
    console.log(`e2e: profile ${scope}`);

    // Another identity's pointer, as an older build kept it for everyone.
    const foreign = "842024" + "ab".repeat(36);
    await browser.execute((s: string, f: string) => {
      localStorage.removeItem(`last-conversation:${s}`);
      localStorage.setItem("last-conversation", JSON.stringify({ hash: "uhCkkforeign", agentKey: f, aiId: "another", title: "Another identity's private plan" }));
    }, scope, foreign);
    await reloadToChat();
    const shown = await browser.execute(() => document.body.innerText);
    expect(shown).not.toContain("Another identity's private plan");
    const left = await browser.execute(() => localStorage.getItem("last-conversation"));
    expect(left).toContain("Another identity's private plan");
    console.log("e2e: another identity's pointer is not offered, and stays for its owner");

    // This profile's own AI: the pointer is taken over and offered.
    await browser.execute((o: string) => {
      localStorage.setItem("last-conversation", JSON.stringify({ hash: "uhCkkmine", agentKey: o, aiId: "mine", title: "This identity's own plan" }));
    }, own);
    await reloadToChat();
    const mine = await browser.execute((s: string) => localStorage.getItem(`last-conversation:${s}`), scope);
    expect(mine).toContain("This identity's own plan");
    expect(await browser.execute(() => localStorage.getItem("last-conversation"))).toBeNull();
    // And it is what the start page offers (the check above alone would
    // pass with no Continue line at all).
    const offered = await browser.execute(() => (document.querySelector('[data-testid="continue-last"]') as HTMLElement | null)?.innerText ?? null);
    console.log(`e2e: the start page offers ${JSON.stringify(offered)}`);
    expect(offered ?? "").toContain("This identity's own plan");
    console.log("e2e: this profile's own pointer is taken over");
  });
});
