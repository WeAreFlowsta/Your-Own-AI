// The app is closed while it is still reading the history into memory's
// index, and started again: the reading goes on from where it stopped, it
// does not begin again. Two launches on the same profile
// (YOAI_E2E_KEEP_PROFILE=1), with a history too long to read in one go:
//   YOAI_E2E_READING_PHASE=stop    reading is let run until a good part is
//                                  read, the count is written down, and the
//                                  run ends there with the reading unfinished
//   YOAI_E2E_READING_PHASE=resume  what was read is still read at the start
//                                  (less one batch at most), and the count
//                                  goes on rising
//   YOAI_E2E_READING_FILE          where the count is written (outside the
//                                  repository)
//   YOAI_E2E_READING_UNTIL         how many messages the first launch reads
//                                  before it stops (default 150)
import * as fs from "node:fs";
import { waitForChat, invoke } from "../models/_helpers";

const PHASE = process.env.YOAI_E2E_READING_PHASE ?? "";
const FILE = process.env.YOAI_E2E_READING_FILE ?? "";
const UNTIL = Number(process.env.YOAI_E2E_READING_UNTIL ?? 150);
// The reader embeds a few passages at a time; a stop can lose the ones in hand.
const ONE_BATCH = 8;

type Status = { read: number; total: number; passages: number; waiting: boolean; why: string | null; rebuilding: boolean };
const status = () => invoke<Status>("memory_reading_status");

describe(`reading goes on after the app is closed, phase ${PHASE}`, () => {
  it(`phase ${PHASE}`, async function () {
    if (!PHASE || !FILE) this.skip();
    this.timeout(3_600_000);
    await waitForChat();
    // The records take a while to open on a profile with a long history.
    let s = await status();
    await browser.waitUntil(async () => { s = await status(); return s.total > 0; }, { timeout: 900_000, interval: 5_000, timeoutMsg: "the reader never counted the history" });
    console.log(`e2e: READING at the start of phase ${PHASE}: read ${s.read} of ${s.total}, ${s.passages} passage(s)${s.rebuilding ? ", rebuilding" : ""}`);
    if (PHASE === "stop") {
      expect(s.total).toBeGreaterThan(UNTIL * 2);
      await invoke("memory_read_now", { on: true });
      await browser.waitUntil(async () => { s = await status(); return s.read >= UNTIL; }, { timeout: 2_700_000, interval: 3_000, timeoutMsg: `fewer than ${UNTIL} messages were read in 45 minutes` });
      console.log(`e2e: READING stopped at: read ${s.read} of ${s.total}, ${s.passages} passage(s)`);
      expect(s.read).toBeLessThan(s.total);
      fs.writeFileSync(FILE, JSON.stringify(s));
    } else if (PHASE === "resume") {
      const was = JSON.parse(fs.readFileSync(FILE, "utf8")) as Status;
      console.log(`e2e: READING was stopped at: read ${was.read} of ${was.total}, ${was.passages} passage(s)`);
      expect(s.rebuilding).toBe(false);
      expect(s.read).toBeGreaterThanOrEqual(was.read - ONE_BATCH);
      expect(s.passages).toBeGreaterThanOrEqual(was.passages - ONE_BATCH);
      const from = s.read;
      await invoke("memory_read_now", { on: true });
      await browser.waitUntil(async () => { s = await status(); return s.read >= Math.min(s.total, from + 20); }, { timeout: 1_800_000, interval: 3_000, timeoutMsg: "the reading did not go on after the restart" });
      console.log(`e2e: READING went on: read ${s.read} of ${s.total}, ${s.passages} passage(s)`);
      expect(s.read).toBeGreaterThan(from);
    } else {
      throw new Error(`no phase ${PHASE}`);
    }
  });
});
