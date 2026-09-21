/**
 * Does THIS message need the AI's tools?
 *
 * An AI that carries a tool used to run every message through a tools
 * session: slower to start, a long prompt, and - on an online model - more
 * paid tokens, for "thanks" as much as for "add this to my notes". The gate
 * sends a message to a session only when it is about a tool:
 *
 *   forced   - the person pressed "Answer again with tools"
 *   named    - the message says the tool's own name ("obsidian", "Logseq");
 *              the name comes from the tool's card, never from a word list
 *   sticky   - this conversation's session has already used a tool, so
 *              "make it bigger" still reaches Blender
 *   blind    - no memory model to judge with: keep the old behavior (a
 *              session), so nothing gets worse for anyone
 *   similar  - it reads like a request for the tool, or at least not
 *              clearly like ordinary chat (below)
 *   direct   - clearly ordinary chat: an ordinary answer
 *
 * HOW LIKENESS IS MEASURED. Measured 2026-09-22 on 28 labeled messages
 * (build-docs apps/your-own-ai/guides/tools/gate-matrix.mjs):
 *  - A message against a tool's DESCRIPTION cannot work with a small memory
 *    model: it cannot tell "about MY notes" from "about notes" ("how do I
 *    take better notes in meetings?" 0.61, "what did I write about the boat
 *    survey?" 0.52; 6 of 28 wrong at the best bar, however it is worded).
 *  - A CONTRAST can: how close the message is to the nearest example REQUEST
 *    for the tool, minus how close it is to the nearest example of ORDINARY
 *    CHAT. Real requests +0.01 .. +0.26, ordinary chat -0.25 .. +0.07. With
 *    the line just under the lowest real request: no request missed, 3 of 28
 *    general questions sent to a session for nothing, every conversational
 *    message ("thanks", "make that shorter", "sum that up") answered direct.
 *  - The helper model as a judge of the close calls made it WORSE, both ways
 *    it was asked (yes / no: 11 of 28 wrong, "yes" to nearly everything; a
 *    two-way sort shown both example lists: 7 of 28, all missed requests).
 *    Not used. A close call goes to a session: the cheaper mistake.
 *  - CLEAN CHECK (gate-matrix-3.mjs, 40 messages written before any result
 *    of them was seen, after both lists were broadened): 0 of 20 real
 *    requests missed, 6 of 20 general questions sent to a session for
 *    nothing. Before the gate that second number was 20 of 20.
 * The example requests are the tool card's own data; a tool without any
 * falls back to its description, leaning toward a session (measured weak:
 * 0 missed, 4 of 6 needless - it fails the safe way).
 *
 * A wrong "direct" costs one click (the reply offers the tools); a wrong
 * "session" costs a slower answer. Every verdict is logged as scores only.
 */

/** Contrast (nearest tool request minus nearest ordinary chat): above the
 *  line = a session; at or below = ordinary chat. Measured: requests
 *  +0.01 .. +0.26, ordinary chat -0.25 .. +0.07 - the line sits under the
 *  lowest real request, because a missed tool is the dearer mistake. */
export const CONTRAST_LINE = -0.02;
/** A tool with no example requests: its description, leaning to a session
 *  (measured: every real request scored 0.46 or more). */
export const DESCRIPTION_BAR = 0.46;

/** What ordinary chat sounds like - the other side of the contrast. About no
 *  tool in particular, and broad on purpose: a kind of message with nothing
 *  here to sit close to hovers near zero and gets a session for nothing (10
 *  examples sent 7 of 12 fresh general questions to a session, 2026-09-22).
 *  None of these is a test message of the matrices - keep it that way. */
export const ORDINARY_CHAT = [
  // facts and explanations
  "what is the population of a country?",
  "who invented this?",
  "when did that happen?",
  "explain this to me",
  "how does this work?",
  "why does that happen?",
  "what does this word mean?",
  "tell me about a topic",
  // how-to and advice
  "how do I get better at something?",
  "how long does it take to cook this?",
  "give me some advice on this",
  "what is the best app for that?",
  "which one should I choose?",
  "what are some good habits for this?",
  // writing and ideas
  "write me a story",
  "help me write a message to someone",
  "draft a reply for me",
  "suggest a name for this",
  "give me some ideas",
  "tell me something funny",
  // numbers and language
  "what is this number times that number?",
  "convert this to another unit",
  "translate this sentence",
  "fix the grammar in this",
  // the world right now
  "what will the weather be like?",
  "what is in the news?",
  // about the conversation itself
  "sum up our conversation",
  "make it shorter",
  "say that again more simply",
  "that is not right",
  "keep going",
  "ok",
  "thank you",
  "hello",
];

export interface GateTool {
  name: string;
  title: string;
  description: string;
  /** Things a person says when they want this tool (the card's data). */
  examples: string[];
}

export type GateReason = "forced" | "named" | "sticky" | "blind" | "similar" | "direct";

export interface GateVerdict {
  session: boolean;
  reason: GateReason;
  /** The best score and the tool it belongs to ("" when nothing was scored). */
  best: number;
  tool: string;
}

/** One tool's likeness to a message. `contrast` when the tool has example
 *  requests (can be negative); `description` otherwise (0 .. 1). */
export interface GateScore {
  tool: string;
  kind: "contrast" | "description";
  score: number;
}

/** The tool a message names, by the tool's own name or title as a whole word. */
export function namedTool(message: string, tools: Pick<GateTool, "name" | "title">[]): string | null {
  const text = message.toLowerCase();
  for (const t of tools) {
    for (const word of [t.name, t.title]) {
      const w = word.trim().toLowerCase();
      if (w.length < 3) continue;
      const at = text.indexOf(w);
      if (at === -1) continue;
      const before = at === 0 ? " " : text[at - 1];
      const after = at + w.length >= text.length ? " " : text[at + w.length];
      if (!/[a-z0-9]/.test(before) && !/[a-z0-9]/.test(after)) return t.name;
    }
  }
  return null;
}

/** Does one score say "a request for this tool"? */
export function isRequest(s: GateScore): boolean {
  return s.kind === "description" ? s.score >= DESCRIPTION_BAR : s.score > CONTRAST_LINE;
}

/** The decision, from what is known. Pure: the scores are handed in. */
export function decide(input: {
  forced: boolean;
  sticky: boolean;
  named: string | null;
  /** null = nothing could be scored (no memory model). */
  scores: GateScore[] | null;
}): GateVerdict {
  const scores = input.scores ?? [];
  // A real request outranks any non-request, whatever scale each is on.
  const top = scores.reduce<GateScore | null>((a, b) => {
    if (!a) return b;
    const ra = isRequest(a), rb = isRequest(b);
    return (rb && !ra) || (rb === ra && b.score > a.score) ? b : a;
  }, null);
  const seen = { best: top?.score ?? 0, tool: top?.tool ?? "" };
  if (input.forced) return { session: true, reason: "forced", ...seen };
  if (input.named) return { session: true, reason: "named", best: seen.best, tool: input.named };
  if (input.sticky) return { session: true, reason: "sticky", ...seen };
  if (input.scores === null) return { session: true, reason: "blind", best: 0, tool: "" };
  if (top && isRequest(top)) return { session: true, reason: "similar", ...seen };
  return { session: false, reason: "direct", ...seen };
}

/** The carried tools as the gate reads them: the words of each tool's card. */
export async function gateTools(names: string[]): Promise<GateTool[]> {
  const { listMcpServers, withCardData, MCP_PRESETS } = await import("./mcp");
  const all = await listMcpServers();
  return names.flatMap((n) => {
    const raw = all.find((x) => x.name === n);
    if (!raw) return [];
    const t = withCardData(raw);
    const card = MCP_PRESETS.find((p) => t.source === `preset:${p.id}`);
    return [{ name: t.name, title: card?.title ?? t.name, description: t.description ?? "", examples: t.examples ?? [] }];
  });
}

// Example requests, ordinary chat and descriptions change rarely: their
// vectors are kept for the life of the page, keyed by the text itself.
const vectors = new Map<string, number[]>();

export async function toolsGate(input: {
  message: string;
  previous: string | undefined;
  tools: GateTool[];
  sticky: boolean;
  forced: boolean;
}): Promise<GateVerdict> {
  const named = namedTool(input.message, input.tools);
  let scores: GateScore[] | null = null;
  // Forced, named and sticky need no scoring - but score anyway, it is
  // cheap, so the log can tune the numbers from every kind of turn.
  try {
    const { embedTexts, embedQuery, cosineSimilarity } = await import("./embeddings");
    const { followUpQuery } = await import("./memory");
    // Requests and chat are things a person SAYS, like the message: they
    // carry the query instruction. A description is a passage: it does not.
    const Q = "Represent this sentence for searching relevant passages: ";
    const said = (t: string) => Q + t;
    const wanted = [
      ...ORDINARY_CHAT.map(said),
      ...input.tools.flatMap((t) => (t.examples.length ? t.examples.map(said) : [`${t.title}. ${t.description}`.trim()])),
    ].filter((t) => !vectors.has(t));
    if (wanted.length) {
      const made = await embedTexts(wanted);
      wanted.forEach((t, i) => { if (made[i]) vectors.set(t, made[i]); });
    }
    const queries = [input.message];
    const together = followUpQuery(input.previous, input.message);
    if (together) queries.push(together);
    const qv = (await Promise.all(queries.map((q) => embedQuery(q)))).filter((v): v is number[] => !!v);
    if (qv.length) {
      const nearest = (texts: string[], q: number[]) =>
        Math.max(0, ...texts.map((t) => { const v = vectors.get(t); return v ? cosineSimilarity(q, v) : 0; }));
      scores = input.tools.map((t): GateScore => {
        if (!t.examples.length) {
          const text = `${t.title}. ${t.description}`.trim();
          return { tool: t.name, kind: "description", score: Math.max(...qv.map((q) => nearest([text], q))) };
        }
        const mine = t.examples.map(said), chat = ORDINARY_CHAT.map(said);
        return { tool: t.name, kind: "contrast", score: Math.max(...qv.map((q) => nearest(mine, q) - nearest(chat, q))) };
      });
    }
  } catch {
    scores = null; // no memory model, or it is busy: decide blind
  }
  const verdict = decide({ forced: input.forced, sticky: input.sticky, named, scores });
  // Scores only - never the message, never a note's words.
  const sign = verdict.best >= 0 ? "+" : "";
  const line = `tools gate: best ${sign}${verdict.best.toFixed(2)}${verdict.tool ? ` (${verdict.tool})` : ""}, line ${CONTRAST_LINE} - ${verdict.session ? "session" : "direct"} (${verdict.reason})`;
  console.log(`[Tools] ${line}`);
  void import("./uiLog").then(({ uiLog }) => uiLog(line)).catch(() => {});
  return verdict;
}
