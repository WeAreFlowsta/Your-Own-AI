// Measures wordings of the replacement judgment on the loaded model, on
// the built app (a development build): each form in the file named by
// YOAI_E2E_JUDGE_FORMS is put to every pair of the shipped examples, once
// per seed, and what was said is written beside the run. Nothing is
// asserted about which form is best: this is a measuring run.
//   forms file: [{ "name": "...", "system": "...", "schema": {...}, "max_tokens": 300 }]
//   YOAI_E2E_JUDGE_SEEDS: "1" (default) or "1,2,3"
import { readFileSync, writeFileSync } from "node:fs";
import { resolve } from "node:path";
import { waitForChat, ask, waitForReplyDone } from "./_helpers";

const FILE = process.env.YOAI_E2E_JUDGE_FORMS ?? "";
const OUT = process.env.YOAI_MEMORY_OUT ?? "";
const SEEDS = (process.env.YOAI_E2E_JUDGE_SEEDS ?? "1").split(",").map((s) => Number(s.trim())).filter((n) => n > 0);
const BASE = "http://127.0.0.1:11435/internal";

type Form = { name: string; system: string; schema: unknown; max_tokens?: number };
type Said = { id: string; expected_yes: boolean; said_yes: boolean | null; reply: string };

describe("wordings of the replacement judgment", () => {
  it("puts each form to every pair and writes what was said", async function () {
    if (!FILE) this.skip();
    this.timeout(6 * 3_600_000);
    const forms = JSON.parse(readFileSync(FILE, "utf8")) as Form[];
    await waitForChat();
    // A reply loads the model the forms are put to.
    await ask("Name one primary color and nothing else.");
    await waitForReplyDone();
    const results: { form: string; seed: number; seconds: number; caught: number; ofYes: number; wrongYes: number; ofNo: number; noAnswer: number; said: Said[] }[] = [];
    for (const form of forms) {
      for (const seed of SEEDS) {
        const r = await fetch(`${BASE}/memory-judge-trial`, {
          method: "POST",
          headers: { "content-type": "application/json" },
          body: JSON.stringify({ system: form.system, schema: form.schema, seed, max_tokens: form.max_tokens ?? 300 }),
        });
        const body = (await r.json()) as { said?: Said[]; seconds?: number; error?: string };
        if (!body.said) {
          console.log(`e2e: JUDGE FORM ${form.name} seed ${seed}: did not run: ${body.error ?? r.status}`);
          continue;
        }
        const yes = body.said.filter((s) => s.expected_yes);
        const no = body.said.filter((s) => !s.expected_yes);
        const row = {
          form: form.name,
          seed,
          seconds: body.seconds ?? 0,
          caught: yes.filter((s) => s.said_yes === true).length,
          ofYes: yes.length,
          wrongYes: no.filter((s) => s.said_yes === true).length,
          ofNo: no.length,
          noAnswer: body.said.filter((s) => s.said_yes === null).length,
          said: body.said,
        };
        results.push(row);
        console.log(`e2e: JUDGE FORM ${row.form} seed ${seed}: caught ${row.caught} of ${row.ofYes}, wrong replaces ${row.wrongYes} of ${row.ofNo}, no answer ${row.noAnswer}, ${row.seconds} s; wrong: ${no.filter((s) => s.said_yes === true).map((s) => s.id).join(", ")}; missed: ${yes.filter((s) => s.said_yes !== true).map((s) => s.id).join(", ")}`);
        if (OUT) writeFileSync(resolve(OUT, "judge-trial.json"), JSON.stringify(results, null, 1));
      }
    }
    expect(results.length).toBeGreaterThan(0);
  });
});
