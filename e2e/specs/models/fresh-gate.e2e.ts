// Does the app know a question needs live information? The router's
// freshness check at the Balanced bar (live_info_need - the check behind
// the "may be out of date" line, and the default dial's online hand-off)
// on two labelled sets: A, the set the references, anchors and bars were
// tuned on, and B, held out. The floors are what the 2026-10-17
// recalibration measured, less a little room: a change that falls below
// them has undone it. Every miss and every false hit is printed.
import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { invoke, waitForChat } from "./_helpers";

type Bench = { fresh: string[]; not: string[] };
const load = (s: string) => JSON.parse(readFileSync(resolve(import.meta.dirname ?? ".", "fresh-bench", `${s}.json`), "utf8")) as Bench;

// Measured 2026-10-17: A 114/119 caught, 12/122 false; B 55/60, 3/60.
// Health questions are never "live" here (they stay on the device), so a
// health-shaped fresh item may count as a miss - none are in the sets.
const FLOORS = { a: { caught: 110, falseMax: 14 }, b: { caught: 52, falseMax: 5 } } as const;

describe("the freshness check", () => {
  before(async () => {
    await waitForChat();
  });

  for (const set of ["a", "b"] as const) {
    it(`set ${set.toUpperCase()}: catches live questions and leaves ordinary ones alone`, async function () {
      this.timeout(900_000);
      const bench = load(set);
      const misses: string[] = [];
      const falseHits: string[] = [];
      const t0 = Date.now();
      for (const q of bench.fresh) if (!(await invoke<boolean>("live_info_need", { text: q }))) misses.push(q);
      for (const q of bench.not) if (await invoke<boolean>("live_info_need", { text: q })) falseHits.push(q);
      const caught = bench.fresh.length - misses.length;
      console.log(`e2e: set ${set}: caught ${caught}/${bench.fresh.length}, ordinary flagged ${falseHits.length}/${bench.not.length} in ${Math.round((Date.now() - t0) / 1000)} s`);
      for (const q of misses) console.log(`e2e:   missed  ${q}`);
      for (const q of falseHits) console.log(`e2e:   FALSE   ${q}`);
      expect(caught).toBeGreaterThanOrEqual(FLOORS[set].caught);
      expect(falseHits.length).toBeLessThanOrEqual(FLOORS[set].falseMax);
    });
  }
});
