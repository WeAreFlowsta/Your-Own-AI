// When a rule is offered, measured on the built app with its own search
// part: an instruction given a second time in other words is offered as a
// rule; a different instruction is not. Made-up pairs. The counts are
// printed whatever they are; the pass marks are 10 of 10 and 0 of 10.
import { waitForChat, invoke } from "./_helpers";

type Heard = { offer: boolean; earlier: string | null; why: string; closest: number | null };

const stamp = Date.now();
let n = 0;
const hear = (text: string) => invoke<Heard>("memory_rule_heard", { input: { text, conversation: `measure-${stamp}-${n}`, at: stamp + n++, rules: [] } });

// The same instruction, said again in other words.
const REPEATED: [string, string][] = [
  ["Always write dates as 3 March 2026.", "Please write the date like 3 March 2026 from now on."],
  ["Never use tables in your replies.", "Do not put tables in your answers, ever."],
  ["From now on, answer in one short paragraph.", "Keep every answer to one short paragraph, always."],
  ["Always give prices in dollars.", "Use dollars for every price from now on."],
  ["Stop using emoji.", "Never use emoji in your replies."],
  ["Always call me Sam, not Samuel.", "From now on, use Sam instead of Samuel when you address me."],
  ["Use British spelling every time.", "Always spell things the British way."],
  ["Always show your working for any sum.", "Each time you calculate something, show the steps."],
  ["Never suggest restaurants that are not vegetarian.", "Only suggest vegetarian restaurants from now on."],
  ["Always ask before you send an email.", "Do not send any email without asking me first."],
];

// Two instructions that are not the same instruction.
const DIFFERENT: [string, string][] = [
  ["Always sign letters with my full name.", "Never quote prices without tax."],
  ["Use metric units every time.", "Always answer in French."],
  ["Never start a reply with a greeting.", "Always list sources at the end."],
  ["From now on, keep code in TypeScript.", "From now on, keep meetings to 30 minutes."],
  ["Always use the 24-hour clock.", "Always put the year first in file names."],
  ["Do not use bullet points.", "Do not use the word leverage."],
  ["Always call my partner Jo.", "Always call my dog Biscuit."],
  ["Never recommend flights before 9 am.", "Never recommend hotels without breakfast."],
  ["Keep summaries under 100 words.", "Keep recipes free of nuts."],
  ["Always reply in a formal tone to clients.", "Always reply in a casual tone to friends."],
];

describe("when a rule is offered", () => {
  it("a repeated instruction is offered, a different one is not (the measure)", async function () {
    this.timeout(600_000);
    await waitForChat();
    // The search part may still be starting: the first measured text waits for it.
    await browser.waitUntil(async () => (await hear(`Always warm up first (${stamp}).`)).why !== "not_measured", { timeout: 180_000, interval: 3_000, timeoutMsg: "no text could be measured: the search part did not start" });

    let offered = 0;
    // How close each second saying was to the nearest thing heard before.
    const near: number[] = [];
    const far: number[] = [];
    const missed: string[] = [];
    for (const [first, again] of REPEATED) {
      const a = await hear(first);
      const b = await hear(again);
      near.push(b.closest ?? 0);
      if (a.why === "first" && b.offer) offered++;
      else missed.push(`${first} / ${again} -> ${a.why}, ${b.why} (${(b.closest ?? 0).toFixed(3)})`);
    }
    let wrong = 0;
    const wrongly: string[] = [];
    for (const [one, other] of DIFFERENT) {
      const a = await hear(one);
      const b = await hear(other);
      far.push(Math.max(a.closest ?? 0, b.closest ?? 0));
      if (a.offer || b.offer) {
        wrong++;
        wrongly.push(`${one} / ${other} -> ${a.why} (${a.earlier ?? ""}), ${b.why} (${b.earlier ?? ""})`);
      }
    }
    console.log(`e2e: rule offers: repeated offered ${offered} of ${REPEATED.length}; different offered ${wrong} of ${DIFFERENT.length}`);
    const fmt = (xs: number[]) => xs.map((x) => x.toFixed(3)).join(" ");
    console.log(`e2e: closeness, repeated pairs (the second saying): ${fmt(near)}`);
    console.log(`e2e: closeness, different pairs (the nearest thing heard before): ${fmt(far)}`);
    if (missed.length) console.log(`e2e: not offered: ${JSON.stringify(missed)}`);
    if (wrongly.length) console.log(`e2e: offered wrongly: ${JSON.stringify(wrongly)}`);
    expect(offered).toBe(REPEATED.length);
    expect(wrong).toBe(0);
  });
});
