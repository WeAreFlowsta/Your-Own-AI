#!/usr/bin/env node
/**
 * Memory harness - asks a question set about a person's past work, saves
 * every reply, and prepares reading sheets on which a reader sorts the
 * replies without knowing which setup produced which.
 *
 *   node tools/memory-harness.mjs sheets --questions Q.md --replies R.jsonl --out DIR [--seed N]
 *       Writes DIR/sheet.txt (for the reader) and DIR/key.json (which
 *       setup each label was; never shown to the reader).
 *   node tools/memory-harness.mjs count --questions Q.md --key DIR/key.json --reader READER.json
 *       Counts a reader's codes per setup, answerable questions and traps apart.
 *   node tools/memory-harness.mjs convert --backup BACKUP.json --out HISTORY.json
 *       Turns a backup export of conversations into the history format the
 *       dev build's loader takes (see src-tauri/src/memory_fixture.rs).
 *   node tools/memory-harness.mjs load --history HISTORY.json [--all AI] [--map Name:AI,...] [--model NAME]
 *       Asks a RUNNING DEV BUILD to write that history into its open
 *       profile as ordinary conversations. Never point this at a profile
 *       that holds real conversations: use a scratch profile.
 *   node tools/memory-harness.mjs ask-list --questions Q.md --out ASK.json [--wording W.json --field NAME] [--only N,N,...]
 *       Writes the list the asking spec reads (e2e/specs/memory/ask.e2e.ts):
 *       [{ n, question }]. A wording file, [{ n, <field>: "..." }], asks
 *       each question in other words than the set's own.
 *   node tools/memory-harness.mjs stores --out STORES.json [--facts FACTS.json] [--recall Name=INDEX.json,...]
 *       Joins a facts list and per-AI indexes of earlier turns into the
 *       stores file a dev build's /internal/memory-stores?do=seed takes.
 *   node tools/memory-harness.mjs find --questions Q.md --history HISTORY.json --out FOUND.json [--wording W.json --field NAME] [--only N,...] [--near 2]
 *       Asks a RUNNING DEV BUILD what memory finds for each question (no
 *       chat model) and counts the questions whose answer's place is among
 *       the first 3 and the first 12 messages found. A place is found when
 *       a message of that conversation within `near` messages of it is.
 *
 * Env: YOAI_URL (default http://127.0.0.1:11435).
 *
 * A question set is a Markdown file, one block per question:
 *
 *   **3. C. What is the free delivery threshold?**
 *   - Answer: $60, from Monday March 9.
 *   - Earlier state: $40 (set January 12).
 *   - Where: conv-b06 seq 0-1
 *
 * The letter is the question's kind; kind F is a trap, with "Honest
 * answer:" in place of "Answer:". A replies file has one JSON object per
 * line: { n, setup, question, answer }. A reader file is JSON,
 * { "<n>": "<one code per label, in label order>" }.
 *
 * The made-up set in e2e/fixtures/memory/ is the only one in this
 * repository. Any other set, and every output, is given by path, and an
 * output folder inside the repository is refused.
 */
import { readFileSync, writeFileSync, mkdirSync } from "node:fs";
import { dirname, join, resolve, sep } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";

const REPO = resolve(dirname(fileURLToPath(import.meta.url)), "..");

/** The reader's codes: right, partly, stale, made up or wrong, said no or
 *  brushed off, said it did not know, wrongly said the search found
 *  nothing, no answer arrived. */
export const CODES = ["R", "P", "S", "M", "X", "N", "F", "T"];

/** How much of a reply a sheet shows. */
const SHOWN = 700;

export function parseQuestions(raw) {
  const text = raw.replace(/\r\n/g, "\n");
  const out = [];
  const block = /^\*\*(\d+)\. ([A-G])\. (.+?)\*\*\n((?:- .*\n?)+)/gm;
  for (const m of text.matchAll(block)) {
    const [, n, kind, q, body] = m;
    const lines = body.trim().split("\n").map((l) => l.slice(2));
    const field = (name) => lines.filter((l) => l.startsWith(name)).map((l) => l.slice(l.indexOf(":") + 1).trim());
    const where = [];
    for (const line of lines.filter((l) => /^(Where|Also|Second)/.test(l))) {
      for (const part of line.split(/;| and (?=conv-)/)) {
        const conversation = part.match(/conv-[0-9a-z]+/)?.[0];
        const seq = part.match(/seq (\d+)(?:\s*[-/]\s*(\d+))?/);
        if (conversation && seq) {
          where.push({ conversation, from: Number(seq[1]), to: Number(seq[2] ?? seq[1]) });
        }
      }
    }
    out.push({
      n: Number(n),
      kind,
      trap: kind === "F",
      q,
      answer: (field("Honest answer")[0] ?? field("Answer:")[0] ?? "").trim(),
      earlier: field("Earlier state"),
      where,
    });
  }
  return out;
}

/** A small seeded generator, so a sheet can be made again exactly. */
function rng(seed) {
  let a = seed >>> 0;
  return () => {
    a = (a + 0x6d2b79f5) >>> 0;
    let t = a;
    t = Math.imul(t ^ (t >>> 15), t | 1);
    t ^= t + Math.imul(t ^ (t >>> 7), t | 61);
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
  };
}

function shuffled(items, random) {
  const a = [...items];
  for (let i = a.length - 1; i > 0; i--) {
    const j = Math.floor(random() * (i + 1));
    [a[i], a[j]] = [a[j], a[i]];
  }
  return a;
}

/** What a reader sees of a reply: the text after any thinking, on one
 *  line, cut at 700 characters with the rest counted. */
function shown(answer) {
  const text = String(answer ?? "").split("</think>").pop().split(/\s+/).filter(Boolean).join(" ");
  return text.length > SHOWN ? `${text.slice(0, SHOWN)} ...(+${text.length - SHOWN})` : text;
}

/** The reading sheet and its key. Replies to one question are shown under
 *  numbered labels in an order shuffled per question; the key alone says
 *  which setup each label was. */
export function buildSheet(questions, replies, { seed = 1 } = {}) {
  const byN = new Map();
  for (const r of replies) {
    if (!byN.has(r.n)) byN.set(r.n, []);
    byN.get(r.n).push(r);
  }
  const order = {};
  let sheet = "";
  for (const q of questions) {
    const rows = byN.get(q.n);
    if (!rows?.length) continue;
    const mixed = shuffled(rows, rng(seed * 100003 + q.n));
    order[q.n] = mixed.map((r) => r.setup);
    const older = q.earlier.length ? ` | OLDER, NOW WRONG: ${q.earlier.join(" / ")}` : "";
    sheet += `=== Q${q.n} kind ${q.kind}${q.trap ? " (TRAP)" : ""} | ANSWER: ${q.answer}${older}\n`;
    sheet += `ASKED: ${mixed[0].question ?? q.q}\n`;
    mixed.forEach((r, i) => {
      sheet += `[${i + 1}] ${shown(r.answer)}\n`;
    });
    sheet += "\n";
  }
  return { sheet, key: { seed, order } };
}

/** A reader's codes counted per setup. Refuses a file that does not give
 *  exactly one known code per label for every question on the sheet. */
export function countCodes(questions, key, reader) {
  const empty = () => Object.fromEntries([...CODES.map((c) => [c, 0]), ["total", 0]]);
  const counts = { answerable: {}, traps: {} };
  for (const q of questions) {
    const setups = key.order[q.n] ?? key.order[String(q.n)];
    if (!setups) continue;
    const given = reader[q.n] ?? reader[String(q.n)];
    if (given === undefined) throw new Error(`Q${q.n}: the reader gave no codes`);
    const codes = String(given).trim().split(/\s+/);
    if (codes.length !== setups.length) {
      throw new Error(`Q${q.n}: expected ${setups.length} codes, got ${codes.length}`);
    }
    codes.forEach((code, i) => {
      if (!CODES.includes(code)) throw new Error(`Q${q.n}: unknown code ${code}`);
      const group = q.trap ? counts.traps : counts.answerable;
      group[setups[i]] ??= empty();
      group[setups[i]][code] += 1;
      group[setups[i]].total += 1;
    });
  }
  return counts;
}

/** A backup export of conversations (one snapshot per conversation, each
 *  with its records) as a history file for the loader. Conversations with
 *  no messages are left out. Times and sequence numbers are kept exactly. */
export function convertBackup(backup) {
  const conversations = [];
  const report = { conversations: 0, messages: 0, empty: 0, other_roles: 0, same_start: [] };
  for (const app of backup.apps ?? []) {
    for (const snap of app.snapshots ?? []) {
      const records = snap.data?.records;
      if (!Array.isArray(records)) continue;
      const head = records.find((r) => r.entryType === "Conversation")?.human_readable;
      if (!head) continue;
      const messages = [];
      for (const r of records) {
        if (r.entryType !== "Message") continue;
        const m = r.human_readable;
        if (m.role !== "user" && m.role !== "assistant") {
          report.other_roles += 1;
          continue;
        }
        messages.push({ role: m.role, text: m.content ?? "", at: m.timestamp, sequence: m.sequence });
      }
      if (messages.length === 0) {
        report.empty += 1;
        continue;
      }
      messages.sort((a, b) => a.sequence - b.sequence || a.at - b.at);
      conversations.push({
        id: snap.label,
        ai: head.ai_personality_name || "AI",
        title: head.title ?? "",
        started_at: head.started_at,
        ...(head.source ? { source: head.source } : {}),
        ...(head.model_used ? { model: head.model_used } : {}),
        messages,
      });
      report.messages += messages.length;
    }
  }
  conversations.sort((a, b) => a.started_at - b.started_at);
  const seen = new Map();
  for (const c of conversations) {
    const k = `${c.ai}\n${c.started_at}`;
    if (seen.has(k)) report.same_start.push([seen.get(k), c.id]);
    else seen.set(k, c.id);
  }
  report.conversations = conversations.length;
  return { history: { v: 1, conversations }, report };
}

/** Whether a path is outside this repository (outputs must be). */
export function outsideRepo(path) {
  const p = resolve(path);
  return p !== REPO && !p.startsWith(REPO + sep);
}

function line(c) {
  const parts = CODES.filter((k) => c[k]).map((k) => `${k}${c[k]}`);
  return `${parts.join(" ") || "-"}  (of ${c.total})`;
}

function args(argv) {
  const out = {};
  for (let i = 0; i < argv.length; i++) {
    if (argv[i].startsWith("--")) out[argv[i].slice(2)] = argv[i + 1]?.startsWith("--") || argv[i + 1] === undefined ? true : argv[++i];
  }
  return out;
}

function need(a, names) {
  for (const n of names) {
    if (typeof a[n] !== "string") {
      console.error(`missing --${n}`);
      process.exit(2);
    }
  }
}

/** The questions to ask, in the set's order: `only` keeps those numbers,
 *  and a wording list replaces a question's text where it has one. */
export function askList(questions, { wording = null, field = "", only = null } = {}) {
  const other = new Map((wording ?? []).map((w) => [Number(w.n), w[field]]));
  const keep = only ? new Set(only.map(Number)) : null;
  if (keep) for (const n of keep) if (!questions.some((q) => q.n === n)) throw new Error(`no question ${n} in the set`);
  return questions
    .filter((q) => !keep || keep.has(q.n))
    .map((q) => {
      const text = other.get(q.n);
      if (wording && (typeof text !== "string" || !text.trim())) throw new Error(`the wording file has no "${field}" for question ${q.n}`);
      return { n: q.n, question: wording ? text.trim() : q.q };
    });
}

/** A date as the loader reads it: microseconds, or a date string. */
function micros(when) {
  if (typeof when === "number") return when;
  const ms = Date.parse(when);
  return Number.isNaN(ms) ? null : ms * 1000;
}

/** Where each question's answer is, as the index names places: the
 *  conversation's start time and a range of sequences. */
export function answerPlaces(questions, history) {
  const starts = new Map(history.conversations.map((c) => [c.id, micros(c.started_at)]));
  return new Map(
    questions.map((q) => [
      q.n,
      q.where.map((w) => {
        if (!starts.has(w.conversation)) throw new Error(`question ${q.n} names ${w.conversation}, which the history does not hold`);
        return { started_at: starts.get(w.conversation), from: w.from, to: w.to };
      }),
    ]),
  );
}

/** The rank (1 = first) at which a found message is at one of the places,
 *  or null. `found` is in rank order: [{ started_at, seq }]. */
export function rankOfPlace(found, places, near = 2) {
  const i = found.findIndex((f) => places.some((p) => f.started_at === p.started_at && f.seq >= p.from - near && f.seq <= p.to + near));
  return i < 0 ? null : i + 1;
}

/** How many answerable questions had their place in the first 3 and the
 *  first 12 found. `ranks`: [{ n, rank }] for answerable questions. */
export function findCounts(ranks) {
  const within = (k) => ranks.filter((r) => r.rank !== null && r.rank <= k).length;
  return { questions: ranks.length, first3: within(3), first12: within(12), missed: ranks.filter((r) => r.rank === null || r.rank > 12).map((r) => r.n) };
}

async function main() {
  const [command, ...rest] = process.argv.slice(2);
  const a = args(rest);
  if (command === "convert") {
    need(a, ["backup", "out"]);
    if (!outsideRepo(a.out)) {
      console.error("the output file must be outside this repository");
      process.exit(2);
    }
    const { history, report } = convertBackup(JSON.parse(readFileSync(a.backup, "utf8")));
    writeFileSync(a.out, JSON.stringify(history));
    console.log(`${report.conversations} conversation(s), ${report.messages} message(s) written to ${a.out}; left out: ${report.empty} with no messages, ${report.other_roles} message(s) that are neither the person's nor the AI's`);
    if (report.same_start.length) {
      console.log(`${report.same_start.length} pair(s) of conversations share a start time; the loader will refuse the file until one of each pair is removed`);
    }
  } else if (command === "load") {
    need(a, ["history"]);
    const q = new URLSearchParams({ path: resolve(a.history) });
    for (const k of ["all", "map", "model"]) if (typeof a[k] === "string") q.set(k, a[k]);
    const base = process.env.YOAI_URL || "http://127.0.0.1:11435";
    const res = await fetch(`${base}/internal/memory-fixture-load?${q}`, { method: "POST" });
    const body = await res.json().catch(() => ({ error: `HTTP ${res.status}` }));
    if (!res.ok || body.error) {
      console.error(`not loaded: ${body.error?.message ?? body.error ?? res.status}`);
      process.exit(1);
    }
    console.log(`${body.conversations} conversation(s), ${body.messages} message(s) written; ${body.skipped} already there`);
  } else if (command === "find") {
    need(a, ["questions", "history", "out"]);
    if (!outsideRepo(a.out)) {
      console.error("the output file must be outside this repository");
      process.exit(2);
    }
    const questions = parseQuestions(readFileSync(a.questions, "utf8"));
    const asked = askList(questions, {
      wording: typeof a.wording === "string" ? JSON.parse(readFileSync(a.wording, "utf8")) : null,
      field: typeof a.field === "string" ? a.field : "",
      only: typeof a.only === "string" ? a.only.split(",").map((n) => n.trim()).filter(Boolean) : null,
    });
    const places = answerPlaces(questions, JSON.parse(readFileSync(a.history, "utf8")));
    const near = Number(a.near ?? 2);
    const base = process.env.YOAI_URL || "http://127.0.0.1:11435";
    const rows = [];
    for (const { n, question } of asked) {
      const res = await fetch(`${base}/internal/memory-search?${new URLSearchParams({ q: question, limit: "12" })}`);
      const body = await res.json().catch(() => ({ error: `HTTP ${res.status}` }));
      if (!res.ok || body.error) {
        console.error(`question ${n}: ${body.error?.message ?? body.error ?? res.status}`);
        process.exit(1);
      }
      const trap = questions.find((q) => q.n === n).trap;
      rows.push({ n, question, trap, rank: trap ? null : rankOfPlace(body.found, places.get(n), near), read: body.read, total: body.total, found: body.found });
    }
    const counts = findCounts(rows.filter((r) => !r.trap));
    writeFileSync(a.out, JSON.stringify({ counts, rows }, null, 1));
    const last = rows[rows.length - 1];
    console.log(`${counts.first3} of ${counts.questions} in the first 3, ${counts.first12} in the first 12; missed: ${counts.missed.join(", ") || "none"}`);
    console.log(`the index had read ${last?.read ?? 0} of ${last?.total ?? 0} messages; details in ${a.out}`);
  } else if (command === "ask-list") {
    need(a, ["questions", "out"]);
    if (!outsideRepo(a.out)) {
      console.error("the output file must be outside this repository");
      process.exit(2);
    }
    const list = askList(parseQuestions(readFileSync(a.questions, "utf8")), {
      wording: typeof a.wording === "string" ? JSON.parse(readFileSync(a.wording, "utf8")) : null,
      field: typeof a.field === "string" ? a.field : "",
      only: typeof a.only === "string" ? a.only.split(",").map((n) => n.trim()).filter(Boolean) : null,
    });
    writeFileSync(a.out, JSON.stringify(list, null, 1));
    console.log(`${list.length} question(s) written to ${a.out}`);
  } else if (command === "stores") {
    need(a, ["out"]);
    if (!outsideRepo(a.out)) {
      console.error("the output file must be outside this repository");
      process.exit(2);
    }
    const stores = { recall: [] };
    if (typeof a.facts === "string") stores.facts = JSON.parse(readFileSync(a.facts, "utf8"));
    for (const pair of typeof a.recall === "string" ? a.recall.split(",") : []) {
      const at = pair.indexOf("=");
      if (at < 1) throw new Error(`--recall takes Name=FILE pairs, not "${pair}"`);
      stores.recall.push({ ai: pair.slice(0, at), entries: JSON.parse(readFileSync(pair.slice(at + 1), "utf8")) });
    }
    writeFileSync(a.out, JSON.stringify(stores));
    console.log(`${stores.facts ? stores.facts.length : "no"} fact(s), ${stores.recall.map((r) => `${r.entries.length} index entries for ${r.ai}`).join(", ") || "no index"} written to ${a.out}`);
  } else if (command === "sheets") {
    need(a, ["questions", "replies", "out"]);
    if (!outsideRepo(a.out)) {
      console.error("the output folder must be outside this repository");
      process.exit(2);
    }
    const questions = parseQuestions(readFileSync(a.questions, "utf8"));
    const replies = readFileSync(a.replies, "utf8").split("\n").filter((l) => l.trim()).map((l) => JSON.parse(l));
    const { sheet, key } = buildSheet(questions, replies, { seed: Number(a.seed ?? 1) });
    mkdirSync(a.out, { recursive: true });
    writeFileSync(join(a.out, "sheet.txt"), sheet);
    writeFileSync(join(a.out, "key.json"), JSON.stringify(key, null, 1));
    console.log(`${Object.keys(key.order).length} question(s) on ${join(a.out, "sheet.txt")}; the key is beside it, not for the reader`);
  } else if (command === "count") {
    need(a, ["questions", "key", "reader"]);
    const questions = parseQuestions(readFileSync(a.questions, "utf8"));
    const counts = countCodes(questions, JSON.parse(readFileSync(a.key, "utf8")), JSON.parse(readFileSync(a.reader, "utf8")));
    for (const [name, group] of [["answerable", counts.answerable], ["traps", counts.traps]]) {
      console.log(name);
      for (const [setup, c] of Object.entries(group)) console.log(`  ${setup.padEnd(20)} ${line(c)}`);
    }
  } else {
    console.error("usage: memory-harness.mjs sheets|count|convert|load|ask-list|stores|find ... (see the top of this file)");
    process.exit(2);
  }
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) await main();
