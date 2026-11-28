// Runs the built-in check on the loaded model, three times with the
// examples shuffled, and saves each run's marks and answers.
//   node e2e/run.mjs models --spec e2e/specs/memory/check.e2e.ts
// YOAI_MEMORY_OUT: folder for check.json (default: a temp folder).
import * as fs from "node:fs";
import * as http from "node:http";
import * as os from "node:os";
import * as path from "node:path";
import { waitForChat, invoke } from "../models/_helpers";

const OUT = path.resolve(process.env.YOAI_MEMORY_OUT ?? fs.mkdtempSync(path.join(os.tmpdir(), "memory-check-")));

/// One run can take longer than a fetch waits: a plain request with no limit.
function post(pathname: string): Promise<any> {
  return new Promise((resolve, reject) => {
    const req = http.request({ host: "127.0.0.1", port: 11435, path: pathname, method: "POST", timeout: 0 }, (res) => {
      let body = "";
      res.on("data", (c) => (body += c));
      res.on("end", () => { try { resolve(JSON.parse(body)); } catch (e) { reject(e); } });
    });
    req.on("error", reject);
    req.end();
  });
}

describe("the built-in check, on the loaded model", () => {
  const runs: any[] = [];
  let model = "";

  it("has a model loaded", async () => {
    await waitForChat();
    model = (await invoke<string | null>("get_current_model")) ?? "";
    expect(model).toBeTruthy();
    fs.mkdirSync(OUT, { recursive: true });
  });

  for (const seed of [1, 2, 3]) {
    it(`run ${seed}`, async () => {
      const r = await post(`/internal/memory-check?seed=${seed}`);
      if (r.error) throw new Error(`memory-check: ${JSON.stringify(r.error)}`);
      const run = r.runs[0];
      runs.push(run);
      const marks = run.verdict.parts.map((p: any) => `${p.part}: ${p.right}/${p.examples} right, ${p.caught}/${p.ofYes} caught, ${p.wrongYes} wrong yes, ${p.passed ? "passed" : "FAILED"}`).join("; ");
      console.log(`e2e: check run ${seed} on ${model} (${r.seconds} s): ${marks}`);
      const unread = run.said.filter((s: any) => s.said_yes === null).length;
      console.log(`e2e: check run ${seed}: ${unread} answer(s) could not be read; wrong: ${run.said.filter((s: any) => s.said_yes !== s.expected_yes).map((s: any) => s.id).join(", ") || "none"}`);
      fs.writeFileSync(path.join(OUT, "check.json"), JSON.stringify({ model, runs }, null, 1));
    });
  }

  it("says whether the verdict held steady", async () => {
    const key = (r: any) => `${r.verdict.mayTidy}/${r.verdict.mayKeep}/${r.verdict.mayClose}`;
    const steady = runs.length === 3 && new Set(runs.map(key)).size === 1;
    console.log(`e2e: check verdicts (tidy/keep/close) per run: ${runs.map(key).join(" | ")}; steady: ${steady}`);
    expect(runs.length).toBe(3);
  });
});
