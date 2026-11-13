// A household whose two computers each hold both copies of a starter AI
// (linked before a joining device's starters folded into the identity's):
// devices-live.sh, with DEVICES_TWINS=1, writes that state into both stored
// lists after its own phases, then runs these two.
//   e (device B driven, A up): B stands the two as one under TWINS_LEADER, and
//     writes a conversation under its own copy's founding key.
//   f (device A driven, B up): A stands them as one under the same leader, and
//     its one AI lists the conversation B wrote under the other founding key.
import { waitForChat, invoke, invokeLong } from "../models/_helpers";

const PHASE = process.env.TWINS_PHASE ?? "";
const NAME = process.env.TWINS_NAME ?? "Teresa";
const LEADER = process.env.TWINS_LEADER ?? "";
const OWN = process.env.TWINS_OWN_AGENT ?? "";
const TITLE = `twin talk from B ${process.env.TWINS_STAMP ?? ""}`;
type Ai = Record<string, any>;

async function stored(): Promise<Ai[]> {
  const path = await invoke<string>("profile_store_path", { name: "ai-data.json" });
  return browser.execute(async (p: string) => {
    const t = (window as any).__TAURI__.core;
    const rid = await t.invoke("plugin:store|load", { path: p, autoSave: false });
    const got = await t.invoke("plugin:store|get", { rid, key: "custom-ais" });
    const list = Array.isArray(got) && got.length === 2 && typeof got[1] === "boolean" ? got[0] : got;
    return Array.isArray(list) ? list : [];
  }, path) as unknown as Promise<Ai[]>;
}

/** Both copies have an agent here and stand as one under LEADER; returns the
 *  one that leads. */
async function oneAi(): Promise<Ai> {
  let pair: Ai[] = [];
  await browser.waitUntil(async () => {
    pair = (await stored()).filter((a) => a.name === NAME && (a.status ?? "active") === "active");
    return pair.length === 2 && pair.every((a) => !!a.agentPubKey);
  }, { timeout: 300_000, interval: 3_000, timeoutMsg: `the two ${NAME}s never both had an agent here` });
  await invoke<number>("fold_twins_now");
  pair = (await stored()).filter((a) => a.name === NAME && (a.status ?? "active") === "active");
  const leader = pair.find((a) => !a.foldedInto)!;
  const folded = pair.find((a) => !!a.foldedInto)!;
  expect(leader).toBeTruthy();
  expect(folded).toBeTruthy();
  console.log(`e2e: twins ${PHASE}: ${String(leader.foundingKey).slice(0, 14)} leads, ${String(folded.foundingKey).slice(0, 14)} is folded into ${String(folded.foldedInto).slice(0, 14)}`);
  expect(leader.foundingKey).toBe(LEADER);
  expect(folded.foldedInto).toBe(LEADER);
  // The person sees one.
  await browser.execute(() => location.reload());
  await browser.pause(2000);
  await waitForChat();
  await (await $('[data-testid="ai-selector"]')).click();
  await browser.pause(1000);
  const shown = await browser.execute((n: string) => [...document.querySelectorAll('[data-testid="ai-option"]')].filter((o) => (o as HTMLElement).dataset.aiLabel === n).length, NAME);
  await browser.keys("Escape");
  expect(shown).toBe(1);
  return leader;
}
const titles = async (agentKey: string) =>
  (await invokeLong<{ title: string | null }[]>("get_conversations", { agentKey, maxAgeSecs: null, caller: "e2e twins" }, 600_000)).map((c) => c.title ?? "");

describe(`doubled starters on two devices, phase ${PHASE}`, () => {
  it("stands the two as one AI, the same one on each device", async function () {
    this.timeout(1_800_000);
    expect(LEADER).toBeTruthy();
    await waitForChat();
    const leader = await oneAi();
    if (PHASE === "e") {
      // Written through B's own copy: it hangs at that copy's founding key.
      expect(OWN).toBeTruthy();
      const hash = await invoke<string>("start_conversation", { agentKey: OWN, aiName: NAME, model: "e2e", title: TITLE, source: null });
      await invoke("record_transcript_entry", { agentKey: OWN, conversationHash: hash, role: "user", content: `${TITLE}: hello`, sequence: 0, model: "e2e", thinking: null, tokens: null, provenance: null, agentLog: null, folderPath: null });
      expect(await titles(leader.agentPubKey)).toContain(TITLE);
      // Time for the record to leave this device before it is stopped.
      await browser.pause(60_000);
    } else {
      // What B wrote under the other founding key shows under A's one AI,
      // once it has crossed (minutes, through the rendezvous).
      let seen: string[] = [];
      await browser.waitUntil(async () => { seen = await titles(leader.agentPubKey); return seen.includes(TITLE); },
        { timeout: 900_000, interval: 15_000, timeoutMsg: `A's ${NAME} never listed the conversation B wrote under its own copy` });
      console.log(`e2e: twins f: A's ${NAME} lists ${seen.length} conversation(s), B's among them`);
    }
  });
});
