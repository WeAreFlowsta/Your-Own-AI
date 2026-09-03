#!/usr/bin/env node
/**
 * Routing battery - a LABELLED set of ~200 realistic queries with the side
 * (online | device | either) each one is EXPECTED to land on under the
 * app's routing design, per mode and per "how much goes online" dial.
 * Run it before and after every router change against a RUNNING DEV BUILD
 * and compare the per-bucket shares:
 *
 *   node tools/route-battery.mjs                 # assert the design, exit 1 on failure
 *   node tools/route-battery.mjs --baseline      # never fail, only print shares
 *   node tools/route-battery.mjs --dry           # print the plan (bucket counts) and exit
 *   node tools/route-battery.mjs --only health   # one bucket (comma list allowed)
 *   node tools/route-battery.mjs --mode offline  # one mode (offline | online-offline)
 *   node tools/route-battery.mjs --json out.json # write every result
 *
 * Env: YOAI_URL (default http://127.0.0.1:11435), YOAI_MATRIX_ENTITLED=0 when
 * the signed-in user cannot use online models (relaxes the "too long" rows),
 * YOAI_BATTERY_DELAY_MS (default 40) between requests.
 *
 * The endpoint is the dev-only GET /internal/route-preview (see
 * src-tauri/src/inference_server.rs) which answers {model, reason} or
 * {error} for mode, q, eagerness, task, difficulty, lean, agent, plan and
 * turn_tokens. Requests run sequentially: the server runs real embedding
 * gates (freshness + medical) per call.
 *
 * DIAL -> EAGERNESS NOTE. The dial (frontier | balanced | local) is the NEW
 * design. Today's server only knows `eagerness`, so each dial is mapped:
 *   frontier -> freshness, balanced -> balanced, local -> privacy
 * and the request ALSO carries `online_share=<dial>` so a newer server can
 * read the dial directly (older servers ignore unknown params). Follow-up
 * turns likewise carry `prev_side` and `prev_task` for a newer server.
 */
import { writeFileSync } from "node:fs";

const BASE = process.env.YOAI_URL || "http://127.0.0.1:11435";
const ENTITLED = process.env.YOAI_MATRIX_ENTITLED !== "0";
const DELAY_MS = Number(process.env.YOAI_BATTERY_DELAY_MS || 40);
const HUGE_TURN = "200000";
const REQUEST_TIMEOUT_MS = 60_000;

const MODES = ["offline", "online-offline"];
const DIALS = ["frontier", "balanced", "local"];
const DIAL_TO_EAGERNESS = { frontier: "freshness", balanced: "balanced", local: "privacy" };

// ---------------------------------------------------------------------------
// Queries. One per line; `hard: true` on a code query bumps its difficulty.
// ---------------------------------------------------------------------------

const EVERGREEN_EASY = [
  "what's a good name for a cat",
  "how do I boil an egg so the yolk stays soft",
  "can you suggest a few names for a bakery",
  "what does the word ubiquitous mean",
  "rewrite this to sound friendlier: please send the report by Friday",
  "how many teaspoons are in a tablespoon",
  "what's the difference between affect and effect",
  "give me a short toast for my friend's birthday",
  "how do I say thank you in Japanese",
  "what's a simple recipe for pancakes",
  "can you explain what photosynthesis is in one paragraph",
  "write a two-line thank-you note for a gift",
  "what's the capital of Australia",
  "how far is the moon from the earth",
  "suggest a fun weekend activity for a rainy day",
  "what's the plural of cactus",
  "tell me a clean joke about programmers",
  "how do I fold a fitted sheet",
  "what are some good houseplants for low light",
  "make this sentence shorter: I was wondering if you might possibly be able to help me with something",
  "what is a haiku",
  "why is the sky blue",
  "how long should I steep green tea",
  "what's a good book for a long flight",
  "what does RSVP stand for",
  "help me think of a title for a photo album from our beach trip",
  "hi, how are you doing today",
  "what's the difference between a crocodile and an alligator",
  "can you give me three synonyms for happy",
  "what should I pack for a weekend camping trip",
  "how many ounces are in a cup",
  "what's an easy way to remember the planets in order",
  "write a short caption for a picture of my dog at the park",
  "what is the boiling point of water in fahrenheit",
  "how do you pronounce quinoa",
  "what's a polite way to decline a meeting invite",
  "give me a quick summary of what a sonnet is",
  "how many days are in a leap year",
  "what colors go well with navy blue",
  "can you recommend a board game for four people",
];

const EVERGREEN_HARD = [
  "prove that the square root of 2 is irrational and explain each step",
  "design a rate limiter for a distributed API that must survive a node failure without double-counting",
  "derive the formula for the sum of the first n squares from first principles",
  "analyze the trade-offs between event sourcing and a traditional relational model for an order system",
  "optimize a schedule for six workers covering three shifts with nobody working two in a row, and prove it is minimal",
  "design a caching strategy for a read-heavy service where writes must be visible within one second",
  "analyze why a greedy algorithm fails for the general knapsack problem and derive the dynamic programming recurrence",
  "prove by induction that every tree with n nodes has exactly n minus 1 edges",
  "design a consistent hashing scheme that keeps rebalancing under five percent when a node joins",
  "derive the expected number of comparisons for quicksort on a random input",
  "analyze the failure modes of a two-phase commit protocol under network partitions",
  "optimize a query plan for a join across three large tables and explain which index each step needs",
  "design a schema for versioned documents where any past version can be reconstructed cheaply",
  "prove that the halting problem is undecidable using a diagonal argument",
  "analyze the security of a login flow that keeps a long-lived refresh token in local storage",
  "derive the closed form of the Fibonacci sequence using the characteristic equation",
  "design an idempotent payment retry mechanism and analyze its edge cases",
  "optimize a matrix multiplication loop for cache locality and explain why the loop order matters",
  "analyze whether a Bloom filter or a cuckoo filter fits a deduplication pipeline better and derive the false-positive rates",
  "prove that the sum of two even numbers is even, then generalize the argument to multiples of k",
  "design a leader election protocol for five nodes that tolerates one crash and prove it cannot elect two leaders",
  "derive the time complexity of Dijkstra's algorithm with a binary heap and explain each term",
  "analyze this argument for logical fallacies: all birds fly, penguins are birds, so penguins must fly at night",
  "design a backpressure strategy for a streaming pipeline where the consumer is ten times slower than the producer",
  "optimize a recursive tree traversal to run without stack overflow on a million nodes and prove the visit order is preserved",
];

const FRESH_KEYWORD = [
  "what's the weather today in Chicago",
  "what's the latest news about the space station",
  "who won the game last night",
  "what is the stock price of the biggest airline right now",
  "what's in the news this week",
  "what are the latest updates on the train strike today",
  "is there a storm warning this week for the gulf coast",
  "what's the current exchange rate for euros to dollars today",
  "who won the election in Ireland",
  "what's trending on social media today",
  "what's the weather forecast for this weekend in Denver",
  "what's the latest version of Python released this month",
  "what happened in the markets today",
  "what's the latest on the wildfire in the news",
  "who won the tennis final this weekend",
  "what are today's headlines about the central bank",
  "is the stock market up or down right now",
  "what's the latest score in the cricket match",
  "what's the weather like in London today",
  "what's the latest news on the new tax rules this week",
];

const FRESH_PARAPHRASE = [
  "which phone did apple announce most recently",
  "is the bay bridge still closed for repairs",
  "which team is on top of the premier league table",
  "how much does a gallon of gas cost in Texas at the moment",
  "has the new metro line opened yet",
  "what is the newest iPhone model",
  "are flights out of Heathrow running normally",
  "who is the prime minister of the United Kingdom",
  "what's the price of bitcoin",
  "has the new Marvel movie come out yet",
  "is the national park open again after the flooding",
  "which browser just shipped a major release",
  "what interest rate did the Federal Reserve set at its last meeting",
  "is it going to rain in Seattle tomorrow",
  "which country most recently joined the European Union",
];

const HEALTH = [
  "can you explain my blood test results, my cholesterol is 240",
  "is it safe to take ibuprofen with my blood pressure medication",
  "my doctor said my A1C is 6.8, what does that mean",
  "what does a high white blood cell count on a lab report usually indicate",
  "I have an MRI scheduled for my knee, what should I expect",
  "my blood pressure reading was 150 over 95 this morning, should I be worried",
  "what are the common side effects of metformin",
  "I've had a persistent cough for three weeks with a mild fever, what could it be",
  "how should I prepare for a fasting blood glucose test",
  "my thyroid TSH came back at 6.2, is that high",
  "can I take antihistamines while on an SSRI",
  "what does elevated ALT on a liver panel mean",
  "my doctor wants to put me on a statin, what are the risks",
  "I've been getting migraines with aura twice a week, what should I ask my neurologist",
  "what's the normal range for hemoglobin in adults",
  "my child has a rash and a low-grade fever after a vaccination, is that normal",
  "what does a low vitamin D level on my bloodwork mean for me",
  "I was diagnosed with prediabetes, what diet changes help",
  "my ECG showed a first-degree AV block, is that serious",
  "can you explain what my bone density scan T-score of minus 1.8 means",
  "what dosage of acetaminophen is safe per day for an adult",
  "I get chest tightness when climbing stairs, should I see a cardiologist",
  "my doctor mentioned my ferritin is low, what does that affect",
  "what are the symptoms of a urinary tract infection and when should I see a doctor",
  "my prescription says take with food, does that mean a full meal",
];

const CODE = [
  "write a Python function that reverses the words in a sentence",
  "explain what a closure is in JavaScript with a small example",
  "why does this Rust code fail to compile: let s = String::from(\"hi\"); let t = s; println!(\"{}\", s);",
  "write a SQL query that returns the top five customers by total order value",
  "how do I read a JSON file in Go",
  "convert this for loop into a list comprehension: for x in nums: if x > 0: out.append(x * 2)",
  "write a bash one-liner that counts lines in all .txt files in a folder",
  "what's the difference between let and var in JavaScript",
  "debug this: my React component re-renders forever when I call setState inside useEffect",
  "write a TypeScript type for a user with an optional email and a required id",
  "explain how async and await work in Python",
  "write a regex that matches a US zip code with an optional four-digit extension",
  "how do I make an HTTP GET request in Rust with reqwest",
  "why is my Java program throwing a NullPointerException on a list that I initialized",
  "write a C function that checks whether a string is a palindrome",
  "explain the difference between an interface and an abstract class in C#",
  { q: "design and implement a thread-safe LRU cache in Java with O(1) get and put", hard: true },
  { q: "refactor a 400-line Python script with global state into testable modules and explain the dependency injection approach", hard: true },
  { q: "write a Rust async worker pool with graceful shutdown and a bounded queue, and analyze its cancellation safety", hard: true },
  { q: "debug a race condition in a Go service where two goroutines update a map, then design the fix", hard: true },
];

const MATH = [
  "solve for x: 3x + 7 = 2x - 5 and check the answer",
  "what is the integral of x squared times e to the x",
  "find the eigenvalues of the matrix [[2, 1], [1, 2]]",
  "how many ways can I arrange 5 books on a shelf if two of them must stay together",
  "what's the probability of rolling at least one six in four rolls of a die",
  "compute the limit of sin(x)/x as x approaches zero and explain why",
  "find the derivative of ln(x) times cos(x)",
  "what's the sum of the geometric series 1 + 1/3 + 1/9 + ... to infinity",
  "solve the system: 2x + y = 7 and x - y = 2",
  "how much do I have after 10 years if I invest 5000 at 4 percent compounded monthly",
];

const REASONING = [
  "three people pay 30 for a room, the clerk refunds 5, the bellhop keeps 2, where did the missing dollar go",
  "if all squares are rectangles and some rectangles are not squares, what can we conclude about a shape that is not a rectangle",
  "a farmer must cross a river with a wolf, a goat and a cabbage, work out the sequence of trips",
  "two trains leave stations 300 miles apart at 60 and 40 mph toward each other, when do they meet and why",
  "I have 8 coins and one is lighter, what is the minimum number of weighings to find it and what's the strategy",
  "which is the better deal: 30 percent off then 20 percent off, or a flat 45 percent off, and why",
  "if it takes 5 machines 5 minutes to make 5 widgets, how long does it take 100 machines to make 100 widgets",
  "a bat and a ball cost 1.10 and the bat costs a dollar more than the ball, what does the ball cost and where do people go wrong",
  "walk me through whether a four-day work week would raise or lower output for a small software team",
  "is it more likely that a randomly picked person is a librarian or a farmer, given they are quiet and tidy, and why",
];

// Normal questions, sent with turn_tokens=200000 (a pasted document).
const LONG_TURN = [
  "summarize the attached document into five bullet points",
  "what are the main themes across everything I pasted above",
  "proofread this chapter for consistency in names and dates",
  "give me the key decisions from these meeting notes",
  "translate the whole text above into plain English",
  "what questions should I ask after reading this contract",
  "find the contradictions between these two reports",
  "outline this manuscript chapter by chapter",
  "list every action item mentioned in the transcript",
  "write a one-page abstract for this thesis",
];

// [bucket of the first turn, first query, short anaphoric follow-up]
const FOLLOWUPS = [
  ["evergreen_easy", "what's a good name for a golden retriever puppy", "more like that please"],
  ["evergreen_easy", "how do I make a simple vinaigrette", "and without mustard?"],
  ["evergreen_easy", "what does the word serendipity mean", "use it in a sentence"],
  ["evergreen_easy", "why is the sky blue", "and why are sunsets red"],
  ["evergreen_easy", "how do rainbows form", "can you get one at night"],
  ["evergreen_easy", "why is the ocean salty", "is every sea the same"],
  ["evergreen_easy", "what causes thunder", "why does it come after the flash"],
  ["evergreen_easy", "suggest three names for a coffee shop", "shorter ones"],
  ["evergreen_easy", "what's the capital of Canada", "and its population?"],
  ["evergreen_hard", "analyze the trade-offs of microservices versus a modular monolith for a ten-person team", "why?"],
  ["evergreen_hard", "prove that there are infinitely many primes", "more on that"],
  ["evergreen_hard", "design a rate limiting scheme for a public API with per-user and global caps", "and the second option?"],
  ["fresh_keyword", "what's the weather today in Boston", "and tomorrow?"],
  ["fresh_keyword", "who won the match last night", "what was the score?"],
  ["fresh_keyword", "what's in the news this week about the housing market", "tell me more"],
  ["fresh_paraphrase", "which team is on top of the league table", "and the second one?"],
  ["fresh_paraphrase", "is the coastal highway still closed", "when does it reopen?"],
  ["health", "my doctor said my cholesterol is high, what does that mean", "what should I eat?"],
  ["health", "what are the side effects of my blood pressure medication amlodipine", "why?"],
  ["health", "I have an MRI on my shoulder next week, how should I prepare", "more on that"],
  ["health", "my blood test shows low iron, what are the symptoms", "shorter please"],
  ["health", "is it ok to take ibuprofen with my antidepressant", "and acetaminophen?"],
  ["code", "write a Python function that checks if a number is prime", "make it faster"],
  ["code", "explain what a JavaScript promise is", "show an example"],
  ["code", "how do I parse a date string in Go", "and format it back?"],
  ["math", "what's the derivative of x cubed times sin x", "and the second derivative?"],
  ["math", "solve 2x + 3 = 11", "why?"],
  ["reasoning", "if 5 cats catch 5 mice in 5 minutes, how long for 100 cats to catch 100 mice", "explain it again more simply"],
  ["reasoning", "which is heavier, a kilogram of feathers or a kilogram of steel, and why do people get it wrong", "shorter please"],
];

// ---------------------------------------------------------------------------
// Labelling: bucket -> request params and expectations.
// ---------------------------------------------------------------------------

const BUCKET_ORDER = [
  "evergreen_easy", "evergreen_hard", "fresh_keyword", "fresh_paraphrase",
  "health", "code", "math", "reasoning", "long_turn", "followup",
];

/** Task the preview is asked with, per bucket. */
function taskFor(bucket) {
  return ["code", "math", "reasoning"].includes(bucket) ? bucket : "general";
}

/** Difficulty the preview is asked with, per bucket (code may opt in to hard). */
function difficultyFor(bucket, hard) {
  if (["evergreen_hard", "math", "reasoning"].includes(bucket)) return "hard";
  return hard ? "hard" : "easy";
}

/** Future `think` expectation; null = not asserted. */
function expectThink(bucket) {
  if (["evergreen_hard", "math", "reasoning"].includes(bucket)) return true;
  if (bucket === "evergreen_easy") return false;
  return null;
}

/**
 * Expected side per bucket, mode and dial: "online" | "device" | "either" |
 * "inherit" (follow-ups: the first turn's OBSERVED side, health -> device).
 */
function expectSide(bucket, mode, dial) {
  if (mode === "offline") return "device";
  switch (bucket) {
    case "health": return "device";
    case "fresh_keyword": return "online";
    case "fresh_paraphrase": return dial === "local" ? "either" : "online";
    case "evergreen_hard": return dial === "local" ? "device" : "online";
    case "evergreen_easy":
    case "code":
    case "math":
    case "reasoning":
      return dial === "frontier" ? "online" : dial === "balanced" ? "either" : "device";
    case "long_turn": return ENTITLED ? "online" : "either";
    case "followup": return "inherit";
    default: throw new Error(`unknown bucket ${bucket}`);
  }
}

/** Build the flat query list: { id, bucket, q, hard, follow?, expect: { think } }. */
function buildQueries() {
  const out = [];
  const add = (bucket, items) =>
    items.forEach((item, i) => {
      const obj = typeof item === "string" ? { q: item } : item;
      out.push({
        id: `${bucket}-${String(i + 1).padStart(2, "0")}`,
        bucket,
        q: obj.q,
        hard: Boolean(obj.hard),
        expect: { think: expectThink(bucket) },
      });
    });
  add("evergreen_easy", EVERGREEN_EASY);
  add("evergreen_hard", EVERGREEN_HARD);
  add("fresh_keyword", FRESH_KEYWORD);
  add("fresh_paraphrase", FRESH_PARAPHRASE);
  add("health", HEALTH);
  add("code", CODE);
  add("math", MATH);
  add("reasoning", REASONING);
  add("long_turn", LONG_TURN);
  FOLLOWUPS.forEach(([firstBucket, q, follow], i) => {
    out.push({
      id: `followup-${String(i + 1).padStart(2, "0")}`,
      bucket: "followup",
      firstBucket,
      q,
      follow,
      hard: false,
      expect: { think: null },
    });
  });
  return out;
}

// ---------------------------------------------------------------------------
// CLI
// ---------------------------------------------------------------------------

function parseArgs(argv) {
  const opts = { baseline: false, dry: false, only: null, mode: null, json: null };
  for (let i = 0; i < argv.length; i++) {
    const a = argv[i];
    if (a === "--baseline") opts.baseline = true;
    else if (a === "--dry") opts.dry = true;
    else if (a === "--dump-json") opts.dumpJson = true;
    else if (a === "--only") opts.only = (argv[++i] || "").split(",").filter(Boolean);
    else if (a === "--mode") opts.mode = argv[++i];
    else if (a === "--json") opts.json = argv[++i];
    else if (a === "--help" || a === "-h") { printUsage(); process.exit(0); }
    else { console.error(`unknown argument: ${a}`); printUsage(); process.exit(2); }
  }
  if (opts.mode && !MODES.includes(opts.mode)) {
    console.error(`--mode must be one of ${MODES.join(" | ")}`);
    process.exit(2);
  }
  if (opts.only) {
    const bad = opts.only.filter((b) => !BUCKET_ORDER.includes(b));
    if (bad.length) {
      console.error(`unknown bucket(s): ${bad.join(", ")}; known: ${BUCKET_ORDER.join(", ")}`);
      process.exit(2);
    }
  }
  return opts;
}

function printUsage() {
  console.log("usage: node tools/route-battery.mjs [--baseline] [--dry] [--only <bucket[,bucket]>] [--mode <offline|online-offline>] [--json <path>]");
}

// ---------------------------------------------------------------------------
// Runner
// ---------------------------------------------------------------------------

const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

/** GET /internal/route-preview; never throws - unreachable becomes a result. */
async function preview(params) {
  const url = `${BASE}/internal/route-preview?${new URLSearchParams(params)}`;
  const ctl = new AbortController();
  const timer = setTimeout(() => ctl.abort(), REQUEST_TIMEOUT_MS);
  try {
    const res = await fetch(url, { signal: ctl.signal });
    return await res.json();
  } catch (e) {
    return { error: `unreachable: ${e.message}`, unreachable: true };
  } finally {
    clearTimeout(timer);
  }
}

/** Raw side from the JSON: online | device | refused | unreachable. */
function classify(r) {
  if (r.unreachable) return "unreachable";
  if (r.error) return "refused";
  return String(r.model).startsWith("online:") ? "online" : "device";
}

/** A refusal is a device outcome in offline mode and for health; otherwise it stands. */
function effectiveSide(raw, mode, bucket) {
  if (raw === "refused" && (mode === "offline" || bucket === "health")) return "device";
  return raw;
}

/** Base params shared by every request for a query. */
function paramsFor(query, mode, dial, bucket) {
  const p = {
    mode,
    q: query.q,
    eagerness: DIAL_TO_EAGERNESS[dial],
    online_share: dial,
    task: taskFor(bucket),
    difficulty: difficultyFor(bucket, query.hard),
    lean: "balanced",
    agent: "0",
    plan: "0",
  };
  if (bucket === "long_turn") p.turn_tokens = HUGE_TURN;
  return p;
}

/**
 * Judge one observation against its expectation. Returns null when it
 * holds, else a short failure label. `either` never fails.
 */
function judge({ expected, side, mode, bucket, r }) {
  if (side === "unreachable") return "endpoint unreachable";
  if (side === "refused") return "refused (no model)";
  const reason = String(r.reason || "");
  if (mode === "offline" && /online/i.test(reason)) return "offline reason mentions online";
  if (expected === "either") return null;
  if (expected !== side) return `expected ${expected}, got ${side}`;
  if (bucket === "long_turn" && expected === "online" && !/too long/.test(reason))
    return "went online without the 'too long' reason";
  return null;
}

/** Future think check; null when the server does not report `think`. */
function judgeThink(expectThinkValue, r) {
  if (expectThinkValue === null || typeof r.think !== "boolean") return null;
  return r.think === expectThinkValue ? null : `expected think=${expectThinkValue}, got ${r.think}`;
}

/** Run one query (and its follow-up, if any) under one mode x dial. */
async function runQuery(query, mode, dial, results) {
  const bucket = query.bucket === "followup" ? query.firstBucket : query.bucket;
  const base = paramsFor(query, mode, dial, bucket);
  const r = await preview(base);
  const raw = classify(r);
  const side = effectiveSide(raw, mode, bucket);
  // A follow-up's first turn is judged under ITS bucket's expectation, so a
  // wrong first turn shows up there and not as a bogus inheritance failure.
  const expected = expectSide(bucket, mode, dial);
  results.push({
    id: query.bucket === "followup" ? `${query.id}.first` : query.id,
    bucket: query.bucket === "followup" ? "followup" : bucket,
    role: query.bucket === "followup" ? "first" : "single",
    mode, dial, q: query.q,
    expected, observed: side, raw,
    model: r.model ?? null, reason: r.reason ?? r.error ?? null, think: r.think ?? null,
    fail: judge({ expected, side, mode, bucket, r }),
    thinkFail: judgeThink(query.expect.think, r),
  });
  if (!query.follow) return;

  await sleep(DELAY_MS);
  const fr = await preview({ ...base, q: query.follow, prev_side: side, prev_task: base.task });
  const fraw = classify(fr);
  const fside = effectiveSide(fraw, mode, bucket);
  // Inheritance: the follow-up lands where the first turn landed, except a
  // health thread which is device regardless. Unreachable/refused first
  // turns cannot anchor an expectation - treat those as `either`.
  const inherited = bucket === "health" ? "device"
    : ["online", "device"].includes(side) ? side : "either";
  results.push({
    id: `${query.id}.follow`,
    bucket: "followup",
    role: "follow",
    mode, dial, q: query.follow,
    expected: inherited, observed: fside, raw: fraw,
    model: fr.model ?? null, reason: fr.reason ?? fr.error ?? null, think: fr.think ?? null,
    fail: judge({ expected: inherited, side: fside, mode, bucket, r: fr }),
    thinkFail: null,
  });
}

// ---------------------------------------------------------------------------
// Reporting
// ---------------------------------------------------------------------------

/** Per-bucket rows: mode, bucket, dial, expected, online share, failures. */
function printTable(results, modes) {
  const rows = [];
  for (const mode of modes)
    for (const bucket of BUCKET_ORDER)
      for (const dial of DIALS) {
        const rs = results.filter((r) =>
          r.mode === mode && r.bucket === bucket && r.dial === dial && r.role !== "first");
        if (!rs.length) continue;
        const online = rs.filter((r) => r.observed === "online").length;
        const fails = rs.filter((r) => r.fail || r.thinkFail).length;
        const expected = bucket === "followup" ? "inherit" : expectSide(bucket, mode, dial);
        rows.push([mode, bucket, dial, expected, `${online}/${rs.length}`, String(fails)]);
      }
  const head = ["mode", "bucket", "dial", "expected", "online", "fails"];
  const widths = head.map((h, i) => Math.max(h.length, ...rows.map((r) => r[i].length)));
  const line = (cols) => cols.map((c, i) => c.padEnd(widths[i])).join("  ");
  console.log("\n" + line(head));
  console.log(widths.map((w) => "-".repeat(w)).join("  "));
  for (const r of rows) console.log(line(r));
}

function printFailures(results) {
  const fails = results.filter((r) => r.fail || r.thinkFail);
  if (!fails.length) { console.log("\nno invariant failures"); return 0; }
  console.log(`\n${fails.length} INVARIANT FAILURES (id | mode | dial | expected | observed | model | reason):`);
  for (const r of fails) {
    const why = [r.fail, r.thinkFail].filter(Boolean).join("; ");
    console.log(`  ${r.id} | ${r.mode} | ${r.dial} | ${r.expected} | ${r.observed} | ${r.model ?? "-"} | ${r.reason ?? "-"}  <- ${why}`);
  }
  return fails.length;
}

function printPlan(queries, modes) {
  const counts = {};
  for (const q of queries) counts[q.bucket] = (counts[q.bucket] || 0) + 1;
  console.log(`route battery plan (dry run) - server ${BASE}, entitled=${ENTITLED}`);
  console.log(`modes: ${modes.join(", ")}   dials: ${DIALS.join(", ")}   (dial -> eagerness: ${
    DIALS.map((d) => `${d}->${DIAL_TO_EAGERNESS[d]}`).join(", ")})`);
  console.log("\nbucket             queries  requests/dial");
  for (const b of BUCKET_ORDER) {
    if (!counts[b]) continue;
    const per = b === "followup" ? counts[b] * 2 : counts[b];
    console.log(`${b.padEnd(18)} ${String(counts[b]).padStart(7)}  ${String(per).padStart(13)}`);
  }
  const perDial = queries.reduce((n, q) => n + (q.follow ? 2 : 1), 0);
  console.log(`\ntotal queries: ${queries.length}   requests per mode x dial: ${perDial}   total requests: ${perDial * modes.length * DIALS.length}`);
}

// ---------------------------------------------------------------------------
// Main
// ---------------------------------------------------------------------------

async function main() {
  const opts = parseArgs(process.argv.slice(2));
  const modes = opts.mode ? [opts.mode] : MODES;
  let queries = buildQueries();
  if (opts.only) queries = queries.filter((q) => opts.only.includes(q.bucket));
  if (!queries.length) { console.error("no queries selected"); process.exit(2); }

  if (opts.dumpJson) {
    // One query table for two harnesses: this script (dev, over HTTP) and the
    // in-app beta matrix's routing leg (Rust, include_str). Regenerate with
    //   node tools/route-battery.mjs --dump-json > src-tauri/route-battery.json
    const table = queries.map((q) => ({
      id: q.id, bucket: q.bucket, q: q.q, follow: q.follow ?? null, firstBucket: q.firstBucket ?? null,
      task: taskFor(q.firstBucket ?? q.bucket), difficulty: difficultyFor(q.firstBucket ?? q.bucket, q.hard),
      think: q.expect.think,
      expect: Object.fromEntries(MODES.map((m) => [m, Object.fromEntries(DIALS.map((d) => [d, expectSide(q.bucket, m, d)]))])),
    }));
    console.log(JSON.stringify({ generated_by: "tools/route-battery.mjs --dump-json", queries: table }, null, 0));
    return 0;
  }
  if (opts.dry) { printPlan(queries, modes); return 0; }

  const started = Date.now();
  const results = [];
  const totalQueries = queries.length * modes.length * DIALS.length;
  let done = 0;
  console.log(`route battery: ${queries.length} queries x ${modes.length} mode(s) x ${DIALS.length} dials against ${BASE} (entitled=${ENTITLED})`);

  for (const mode of modes)
    for (const dial of DIALS)
      for (const query of queries) {
        await runQuery(query, mode, dial, results);
        done++;
        const last = results[results.length - 1];
        if (last.observed === "unreachable" && done === 1) {
          console.error(`\n${BASE} unreachable (${last.reason}). Is a DEV build running? The preview endpoint is dev-only.`);
          return 2;
        }
        if (done % 25 === 0 || done === totalQueries) {
          const secs = ((Date.now() - started) / 1000).toFixed(0);
          console.log(`  ${String(done).padStart(4)}/${totalQueries}  ${mode}/${dial}  ${secs}s`);
        }
        await sleep(DELAY_MS);
      }

  printTable(results, modes);
  const failures = printFailures(results);
  const secs = ((Date.now() - started) / 1000).toFixed(1);
  console.log(`\n${results.length} requests in ${secs}s`);

  if (opts.json) {
    writeFileSync(opts.json, JSON.stringify({
      base: BASE, entitled: ENTITLED, startedAt: new Date(started).toISOString(),
      seconds: Number(secs), modes, dials: DIALS, dialToEagerness: DIAL_TO_EAGERNESS,
      results,
    }, null, 2));
    console.log(`wrote ${opts.json}`);
  }

  if (failures && !opts.baseline) return 1;
  if (failures && opts.baseline) console.log("(--baseline: failures reported, not fatal)");
  return 0;
}

process.exit(await main());
