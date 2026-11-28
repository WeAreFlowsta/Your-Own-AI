// Tests for the parts of tools/memory-harness.mjs that need no running app.
// Run: node --test tools/memory-harness.test.mjs
import test from "node:test";
import assert from "node:assert/strict";
import { mkdtempSync, readFileSync, writeFileSync, existsSync } from "node:fs";
import { tmpdir } from "node:os";
import { join, dirname, basename } from "node:path";
import { fileURLToPath } from "node:url";
import {
  parseQuestions,
  buildSheet,
  countCodes,
  outsideRepo,
  convertBackup,
  askList,
  answerPlaces,
  rankOfPlace,
  findCounts,
  CODES,
} from "./memory-harness.mjs";

const here = dirname(fileURLToPath(import.meta.url));
const repo = join(here, "..");
const fixture = join(repo, "e2e", "fixtures", "memory");

test("the made-up question set parses: kinds, answers, places, traps", () => {
  const qs = parseQuestions(readFileSync(join(fixture, "questions.md"), "utf8"));
  assert.equal(qs.length, 15);
  assert.deepEqual(qs.map((q) => q.n), Array.from({ length: 15 }, (_, i) => i + 1));
  const q3 = qs.find((q) => q.n === 3);
  assert.equal(q3.kind, "C");
  assert.equal(q3.q, "What is the free delivery threshold?");
  assert.match(q3.answer, /\$60/);
  assert.deepEqual(q3.earlier, ["$40 (set January 12)."]);
  assert.deepEqual(q3.where, [{ conversation: "conv-b06", from: 0, to: 1 }]);
  const traps = qs.filter((q) => q.trap);
  assert.deepEqual(traps.map((q) => q.n), [13, 14, 15]);
  assert.ok(traps.every((q) => q.where.length === 0 && q.answer.length > 0));
  assert.equal(qs.filter((q) => !q.trap).length, 12);
});

test("every place a question names exists in the made-up history", () => {
  const history = JSON.parse(readFileSync(join(fixture, "history.json"), "utf8"));
  const sizes = new Map(history.conversations.map((c) => [c.id, c.messages.length]));
  const starts = history.conversations.map((c) => c.started_at);
  assert.equal(new Set(starts).size, starts.length, "every conversation has its own start time");
  for (const q of parseQuestions(readFileSync(join(fixture, "questions.md"), "utf8"))) {
    for (const w of q.where) {
      assert.ok(sizes.has(w.conversation), `Q${q.n}: ${w.conversation}`);
      assert.ok(w.to < sizes.get(w.conversation), `Q${q.n}: seq ${w.to}`);
    }
  }
});

const questions = parseQuestions(
  [
    "**1. A. Which mill?**",
    "- Answer: Hartwell Mills.",
    "- Where: conv-b02 seq 2-3",
    "",
    "**2. C. What threshold?**",
    "- Answer: $60.",
    "- Earlier state: $40.",
    "- Where: conv-b06 seq 0",
    "",
    "**3. F. Which accountant?**",
    "- Honest answer: None is in the history.",
    "",
  ].join("\n"),
);
const replies = [
  { n: 1, setup: "today", question: "Which mill?", answer: "Dunmore Grain." },
  { n: 1, setup: "beside-question", question: "Which mill?", answer: "<think>hm</think>Hartwell Mills, as you said on 2026-01-20." },
  { n: 2, setup: "today", question: "What threshold?", answer: "$40." },
  { n: 2, setup: "beside-question", question: "What threshold?", answer: "x".repeat(900) },
  { n: 3, setup: "today", question: "Which accountant?", answer: "Bellamy and Co." },
  { n: 3, setup: "beside-question", question: "Which accountant?", answer: "I searched and found no accountant." },
];

test("a reading sheet never names a setup, and its key is kept apart", () => {
  const { sheet, key } = buildSheet(questions, replies, { seed: 7 });
  for (const name of ["today", "beside-question"]) {
    assert.ok(!sheet.includes(name), `the sheet names the setup "${name}"`);
  }
  assert.match(sheet, /=== Q1 kind A \| ANSWER: Hartwell Mills\./);
  assert.match(sheet, /=== Q2 kind C \| ANSWER: \$60\. \| OLDER, NOW WRONG: \$40\./);
  assert.match(sheet, /=== Q3 kind F \(TRAP\) \| ANSWER: None is in the history\./);
  assert.match(sheet, /ASKED: Which mill\?/);
  // Neutral labels, one line per reply.
  assert.equal((sheet.match(/^\[\d\] /gm) || []).length, 6);
  // Thinking is not shown; a long reply is cut at 700 characters with the rest counted.
  assert.ok(!sheet.includes("<think>") && !sheet.includes("hm</think>"));
  assert.ok(sheet.includes("x".repeat(700) + " ...(+200)"));
  assert.ok(!sheet.includes("x".repeat(701)));
  // The key says which setup each label was, per question.
  assert.deepEqual(Object.keys(key.order).sort(), ["1", "2", "3"]);
  for (const n of ["1", "2", "3"]) {
    assert.deepEqual([...key.order[n]].sort(), ["beside-question", "today"]);
  }
  assert.equal(key.seed, 7);
});

test("the same seed gives the same sheet, and labels are shuffled per question", () => {
  const a = buildSheet(questions, replies, { seed: 7 });
  const b = buildSheet(questions, replies, { seed: 7 });
  assert.equal(a.sheet, b.sheet);
  assert.deepEqual(a.key, b.key);
  const many = [];
  for (let n = 1; n <= 40; n++) {
    many.push({ n, setup: "today", question: "q", answer: "a" }, { n, setup: "beside-question", question: "q", answer: "b" });
  }
  const qs = parseQuestions(many.filter((_, i) => i % 2 === 0).map((r) => `**${r.n}. A. q**\n- Answer: a.\n`).join("\n"));
  const { key } = buildSheet(qs, many, { seed: 3 });
  const firsts = new Set(Object.values(key.order).map((o) => o[0]));
  assert.equal(firsts.size, 2, "the first label is not always the same setup");
});

test("a reader's codes are counted per setup, answerable and traps apart", () => {
  const { key } = buildSheet(questions, replies, { seed: 7 });
  // The reader gives one code per label, in label order, for each question.
  const code = { today: { 1: "M", 2: "S", 3: "M" }, "beside-question": { 1: "R", 2: "P", 3: "R" } };
  const reader = {};
  for (const n of ["1", "2", "3"]) {
    reader[n] = key.order[n].map((setup) => code[setup][n]).join(" ");
  }
  const counts = countCodes(questions, key, reader);
  assert.deepEqual(counts.answerable.today, { R: 0, P: 0, S: 1, M: 1, X: 0, N: 0, F: 0, T: 0, total: 2 });
  assert.deepEqual(counts.answerable["beside-question"], { R: 1, P: 1, S: 0, M: 0, X: 0, N: 0, F: 0, T: 0, total: 2 });
  assert.deepEqual(counts.traps.today, { R: 0, P: 0, S: 0, M: 1, X: 0, N: 0, F: 0, T: 0, total: 1 });
  assert.deepEqual(counts.traps["beside-question"], { R: 1, P: 0, S: 0, M: 0, X: 0, N: 0, F: 0, T: 0, total: 1 });
});

test("a reader file with a wrong code or a missing label is refused, never guessed", () => {
  const { key } = buildSheet(questions, replies, { seed: 7 });
  assert.throws(() => countCodes(questions, key, { 1: "R Q", 2: "R R", 3: "R R" }), /Q1.*code/);
  assert.throws(() => countCodes(questions, key, { 1: "R", 2: "R R", 3: "R R" }), /Q1.*2 codes/);
  assert.throws(() => countCodes(questions, key, { 1: "R R", 2: "R R" }), /Q3/);
  assert.deepEqual(CODES, ["R", "P", "S", "M", "X", "N", "F", "T"]);
});

test("an output folder inside the repository is refused", () => {
  assert.equal(outsideRepo(join(repo, "tools", "out")), false);
  assert.equal(outsideRepo(repo), false);
  assert.equal(outsideRepo(join(repo, "..", basename(repo), "e2e")), false);
  const out = mkdtempSync(join(tmpdir(), "memory-harness-"));
  assert.equal(outsideRepo(out), true);
  writeFileSync(join(out, "probe.txt"), "x");
  assert.ok(existsSync(join(out, "probe.txt")));
});

test("a backup export becomes a history file: times and sequences kept, empty conversations left out", () => {
  const record = (entryType, human_readable) => ({ entryType, human_readable });
  const backup = {
    apps: [
      {
        snapshots: [
          { label: "summary", data: { conversations: 3 } },
          {
            label: "conv-0a1b2c3d4e5f",
            data: {
              records: [
                record("Message", { role: "assistant", content: "Forty bags.", sequence: 1, timestamp: 1767225960000001, model: "m" }),
                record("Conversation", { ai_personality_name: "Juno", title: "Flour", started_at: 1767225900000123, source: null, model_used: "a-model.gguf" }),
                record("Message", { role: "user", content: "How many bags?", sequence: 0, timestamp: 1767225900000456, model: "m" }),
                record("Message", { role: "system", content: "x", sequence: 2, timestamp: 1767225970000000 }),
                record("GroundingAnnotation", { target: "x" }),
              ],
            },
          },
          { label: "conv-00000000ffff", data: { records: [record("Conversation", { ai_personality_name: "Juno", title: "", started_at: 1767225900000123, source: "some-program/1.0" })] } },
          {
            label: "conv-9f8e7d6c5b4a",
            data: {
              records: [
                record("Conversation", { ai_personality_name: "Juno", title: "Session", started_at: 1767000000000000, source: "some-program/1.0", model_used: "b.gguf" }),
                record("Message", { role: "user", content: null, sequence: 0, timestamp: 1767000000000001 }),
              ],
            },
          },
        ],
      },
    ],
  };
  const { history, report } = convertBackup(backup);
  assert.deepEqual(report, { conversations: 2, messages: 3, empty: 1, other_roles: 1, same_start: [] });
  assert.deepEqual(history.conversations.map((c) => c.id), ["conv-9f8e7d6c5b4a", "conv-0a1b2c3d4e5f"], "oldest first");
  const flour = history.conversations[1];
  assert.deepEqual(flour, {
    id: "conv-0a1b2c3d4e5f",
    ai: "Juno",
    title: "Flour",
    started_at: 1767225900000123,
    model: "a-model.gguf",
    messages: [
      { role: "user", text: "How many bags?", at: 1767225900000456, sequence: 0 },
      { role: "assistant", text: "Forty bags.", at: 1767225960000001, sequence: 1 },
    ],
  });
  assert.equal(history.conversations[0].source, "some-program/1.0");
  assert.equal(history.conversations[0].messages[0].text, "");
});

test("two conversations with messages and one start time are reported, not hidden", () => {
  const conv = (label) => ({
    label,
    data: {
      records: [
        { entryType: "Conversation", human_readable: { ai_personality_name: "Juno", title: "t", started_at: 5 } },
        { entryType: "Message", human_readable: { role: "user", content: "x", sequence: 0, timestamp: 6 } },
      ],
    },
  });
  const { report } = convertBackup({ apps: [{ snapshots: [conv("conv-aa"), conv("conv-bb")] }] });
  assert.deepEqual(report.same_start, [["conv-aa", "conv-bb"]]);
});

test("an ask list keeps the set's order, takes other wordings, and refuses a missing one", () => {
  assert.deepEqual(askList(questions), [
    { n: 1, question: "Which mill?" },
    { n: 2, question: "What threshold?" },
    { n: 3, question: "Which accountant?" },
  ]);
  const wording = [{ n: 3, loose: " who does the books? " }, { n: 1, loose: "where does the flour come from" }];
  assert.deepEqual(askList(questions, { wording, field: "loose", only: ["3", "1"] }), [
    { n: 1, question: "where does the flour come from" },
    { n: 3, question: "who does the books?" },
  ]);
  assert.throws(() => askList(questions, { wording, field: "loose" }), /question 2/);
  assert.throws(() => askList(questions, { only: [9] }), /no question 9/);
});

test("a found message counts when it is at the answer's place, give or take two messages", () => {
  const history = {
    conversations: [
      { id: "conv-b02", started_at: "2026-01-20T09:00:00Z", messages: [] },
      { id: "conv-b06", started_at: 1767960000000000, messages: [] },
    ],
  };
  const places = answerPlaces(questions, history);
  assert.deepEqual(places.get(1), [{ started_at: Date.parse("2026-01-20T09:00:00Z") * 1000, from: 2, to: 3 }]);
  assert.deepEqual(places.get(2), [{ started_at: 1767960000000000, from: 0, to: 0 }]);
  assert.deepEqual(places.get(3), [], "a trap has no place");
  const b02 = places.get(1)[0].started_at;
  const found = [
    { started_at: 1767960000000000, seq: 7 },
    { started_at: b02, seq: 6 },
    { started_at: b02, seq: 5 },
    { started_at: b02, seq: 2 },
  ];
  assert.equal(rankOfPlace(found, places.get(1)), 3, "seq 5 is within two of 2-3");
  assert.equal(rankOfPlace(found, places.get(1), 0), 4);
  assert.equal(rankOfPlace(found, places.get(2)), null, "seq 7 of the right conversation is too far from seq 0");
  assert.equal(rankOfPlace([], places.get(1)), null);
  assert.throws(() => answerPlaces(questions, { conversations: [] }), /conv-b02/);
  assert.deepEqual(findCounts([{ n: 1, rank: 3 }, { n: 2, rank: null }, { n: 4, rank: 12 }, { n: 5, rank: 13 }]), { questions: 4, first3: 1, first12: 2, missed: [2, 5] });
});

test("the person's turn is put together in one order, with one marker", async () => {
  // The page's own function, read from its source (it has no other imports).
  const ts = (await import("typescript")).default;
  const src = readFileSync(join(repo, "src", "utils", "turnWire.ts"), "utf8");
  const js = ts.transpileModule(src, { compilerOptions: { module: "ESNext", target: "ES2020" } }).outputText;
  const { assembleUserTurn } = await import("data:text/javascript;base64," + Buffer.from(js).toString("base64"));
  const q = "Which mill do we use?";
  assert.equal(assembleUserTurn({ question: q }), q, "nothing rides along: the question as typed");
  assert.equal(assembleUserTurn({ question: q, before: "", after: "  " }), q);
  // The memory block alone: the measured form, no marker.
  assert.equal(assembleUserTurn({ question: q, before: "[From memory. x]" }), `[From memory. x]\n\n${q}`);
  assert.equal(assembleUserTurn({ question: q, before: "[From memory. x]", after: "[From memory, the state record y]" }), `[From memory. x]\n\n${q}\n\n[From memory, the state record y]`);
  // A file alone is what it was before memory came beside it.
  assert.equal(assembleUserTurn({ question: q, file: "[file]" }), `[file]\n\n[User question]\n${q}`);
  // Everything: file, pages, document passages, memory, the marker once, the question, the note.
  const all = assembleUserTurn({ file: "[file]", pages: "[pages]", docs: "[docs]", before: "[From memory. x]", question: q, after: "[note]" });
  assert.equal(all, `[file]\n\n[pages]\n\n[docs]\n\n[From memory. x]\n\n[User question]\n${q}\n\n[note]`);
  assert.equal(all.split("[User question]").length, 2, "the marker stands once");
  assert.ok(all.indexOf("[From memory. x]") < all.indexOf(q) && all.indexOf(q) < all.indexOf("[note]"));
});
