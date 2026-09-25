// The tools gate's rules, without a model: a named tool, the order of the
// reasons, the contrast line, and deciding blind. Runs in `npm run build`.
import { readFileSync } from "node:fs";
import { transformSync } from "esbuild";

const src = readFileSync("src/utils/toolsGate.ts", "utf8");
const cut = src.slice(src.indexOf("export const CONTRAST_LINE"), src.indexOf("/** The carried tools as the gate reads them"));
const js = transformSync(cut.replace(/export /g, ""), { loader: "ts" }).code;
const { namedTool, decide, aboutMyThings, CONTRAST_LINE, DESCRIPTION_BAR, ORDINARY_CHAT } = new Function(
  `${js}; return { namedTool, decide, aboutMyThings, CONTRAST_LINE, DESCRIPTION_BAR, ORDINARY_CHAT };`,
)();

const tools = [
  { name: "obsidian", title: "Obsidian" },
  { name: "blender", title: "Blender" },
  { name: "fetch", title: "fetch" },
];
let bad = 0, n = 0;
const eq = (name, got, want) => {
  n++;
  if (JSON.stringify(got) !== JSON.stringify(want)) {
    console.error(`tools gate: "${name}" gave ${JSON.stringify(got)}, wanted ${JSON.stringify(want)}`);
    bad++;
  }
};
eq("names a tool", namedTool("add this to my Obsidian notes", tools), "obsidian");
eq("names it with punctuation", namedTool("Is it in blender?", tools), "blender");
eq("a longer word is not the tool", namedTool("my blenders are broken", tools), null);
eq("inside another word", namedTool("prefetching is slow", tools), null);
eq("names nothing", namedTool("thanks, that helps", tools), null);
eq("ordinary chat has examples", ORDINARY_CHAT.length >= 8, true);

const c = (tool, score) => ({ tool, kind: "contrast", score });
const d = (tool, score) => ({ tool, kind: "description", score });
const r = (i) => { const v = decide(i); return `${v.session ? "session" : "direct"}:${v.reason}:${v.tool}`; };
const base = { forced: false, sticky: false, named: null };
eq("clearly ordinary chat", r({ ...base, scores: [c("obsidian", CONTRAST_LINE - 0.1)] }), "direct:direct:obsidian");
eq("on the line is still chat", r({ ...base, scores: [c("obsidian", CONTRAST_LINE)] }), "direct:direct:obsidian");
eq("just over the line", r({ ...base, scores: [c("obsidian", CONTRAST_LINE + 0.01)] }), "session:similar:obsidian");
eq("clearly a request", r({ ...base, scores: [c("obsidian", 0.2)] }), "session:similar:obsidian");
eq("a request for one tool beats chat for another", r({ ...base, scores: [c("obsidian", -0.2), c("blender", 0.05)] }), "session:similar:blender");
eq("no examples: description over its bar", r({ ...base, scores: [d("fetch", DESCRIPTION_BAR)] }), "session:similar:fetch");
eq("no examples: description under its bar", r({ ...base, scores: [d("fetch", DESCRIPTION_BAR - 0.01)] }), "direct:direct:fetch");
eq("a big description score does not outrank a real request", r({ ...base, scores: [d("fetch", 0.3), c("obsidian", 0.05)] }), "session:similar:obsidian");
eq("named beats ordinary chat", r({ ...base, named: "obsidian", scores: [c("obsidian", -0.2)] }), "session:named:obsidian");
eq("sticky keeps the session", r({ ...base, sticky: true, scores: [c("obsidian", -0.2)] }), "session:sticky:obsidian");
eq("forced beats everything", r({ forced: true, sticky: false, named: null, scores: [c("obsidian", -0.2)] }), "session:forced:obsidian");
eq("no memory model: the old behavior", r({ ...base, scores: null }), "session:blind:");
eq("no memory model, but named", r({ ...base, named: "blender", scores: null }), "session:named:blender");
eq("no tools scored", r({ ...base, scores: [] }), "direct:direct:");
// The world right now skips the tools; the person's own things do not.
eq("live web beats a close call", r({ ...base, liveWeb: true, scores: [c("obsidian", 0.04)] }), "direct:live-web:obsidian");
eq("naming the tool beats live web", r({ ...base, liveWeb: true, named: "obsidian", scores: [c("obsidian", 0.04)] }), "session:named:obsidian");
eq("a tool conversation beats live web", r({ ...base, liveWeb: true, sticky: true, scores: [c("obsidian", 0.04)] }), "session:sticky:obsidian");
eq("forced beats live web", r({ forced: true, sticky: false, named: null, liveWeb: true, scores: [c("obsidian", 0.04)] }), "session:forced:obsidian");
eq("the world: not mine", aboutMyThings("what's the latest in the middle east?"), false);
eq("tell me is not mine", aboutMyThings("tell me the headlines tonight"), false);
eq("my notes are mine", aboutMyThings("what did I add to my notes today?"), true);
eq("a journal page is mine", aboutMyThings("read me today's journal page"), true);
eq("a daily note is mine", aboutMyThings("put this in today's daily note"), true);
eq("inside another word does not count", aboutMyThings("is Miami sunny right now?"), false);

if (bad) process.exit(1);
console.log(`tools gate check: ${n} cases pass`);
