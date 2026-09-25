// The online catalog and the app agree on who does each routing job.
// The catalog (Firestore, served at the proxy's public /v1/models) names the
// default model for each routing slot; the app keeps a baked floor for an
// older catalog (router.rs DEFAULT_*) and the Online Models page's badge
// floor (OnlineModels.tsx BAKED_SLOTS). A catalog edit that leaves a job
// with no model, gives it two, or retires a model the app still falls back
// to, would route silently to the capability fallback - this says so.
// No sign-in needed. Run after every catalog seed, and weekly in CI:
//   node scripts/check-online-catalog.mjs [proxy-url]
import { readFileSync } from "node:fs";

const PROXY = process.argv[2] ?? "https://yoai-model-proxy-386500392150.us-central1.run.app";
const SLOTS = ["everyday", "fresh", "hard_code", "hard_general", "agent", "plan"];
const res = await fetch(`${PROXY}/v1/models`);
if (!res.ok) {
  console.error(`online catalog check: ${PROXY}/v1/models answered ${res.status}`);
  process.exit(1);
}
const models = (await res.json()).data ?? [];
const ids = new Set(models.map((m) => m.id));
let bad = 0;
const fail = (msg) => { bad++; console.error(`online catalog check: ${msg}`); };

// 1. Every routing job has exactly one model in the served catalog.
for (const slot of SLOTS) {
  const holders = models.filter((m) => (m.routing?.slots ?? []).includes(slot)).map((m) => m.id);
  if (holders.length === 0) fail(`no model does "${slot}" - routing would fall back to capability scores`);
  if (holders.length > 1) fail(`"${slot}" has ${holders.length} models (${holders.join(", ")}) - the router takes the first`);
}
// 2. A web-search job goes to a web-search model.
for (const m of models.filter((m) => (m.routing?.slots ?? []).includes("fresh"))) {
  if (m.category !== "web_search" && !(m.categories ?? []).includes("web_search")) fail(`"fresh" is held by ${m.id}, which is not a web-search model`);
}
// 3. The app's baked floors name models the catalog still serves.
const router = readFileSync("src-tauri/src/router.rs", "utf8");
const floors = [...router.matchAll(/const (DEFAULT_[A-Z_]+): &str = "online:([^"]+)";/g)];
if (floors.length < SLOTS.length) fail(`found ${floors.length} baked defaults in router.rs, expected ${SLOTS.length} - has their shape changed?`);
for (const [, name, id] of router.matchAll(/const (DEFAULT_[A-Z_]+): &str = "online:([^"]+)";/g)) {
  if (!ids.has(id)) fail(`router.rs ${name} = ${id}, which the catalog no longer serves`);
}
const page = readFileSync("src/components/OnlineModels.tsx", "utf8");
const baked = page.slice(page.indexOf("const BAKED_SLOTS"), page.indexOf("};", page.indexOf("const BAKED_SLOTS")));
for (const [, id] of baked.matchAll(/'online:([^']+)'/g)) {
  if (!ids.has(id)) fail(`OnlineModels.tsx BAKED_SLOTS names ${id}, which the catalog no longer serves`);
}
// 4. The baked floors agree with the catalog's own slots (drift = the badge
//    and the router disagree on an older catalog).
for (const [, name, id] of router.matchAll(/const (DEFAULT_[A-Z_]+): &str = "online:([^"]+)";/g)) {
  const slot = name.replace("DEFAULT_", "").toLowerCase();
  const m = models.find((x) => x.id === id);
  if (m && SLOTS.includes(slot) && !(m.routing?.slots ?? []).includes(slot)) {
    fail(`router.rs ${name} = ${id}, but the catalog gives "${slot}" to ${models.filter((x) => (x.routing?.slots ?? []).includes(slot)).map((x) => x.id).join(", ") || "nobody"}`);
  }
}
if (bad) process.exit(1);
console.log(`online catalog check: ${models.length} models, every job has one, the app's floors match (${PROXY})`);
