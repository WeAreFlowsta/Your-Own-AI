#!/usr/bin/env node
/**
 * Routing invariant sweep - walks every mode x task x difficulty x
 * eagerness x lean x role combination against a RUNNING DEV BUILD's
 * /internal/route-preview and asserts the promises:
 *
 *   1. offline / my-hardware modes NEVER resolve to an online model.
 *   2. privacy eagerness NEVER resolves online, in any mode.
 *   3. a health question NEVER auto-routes online (chat roles).
 *   4. agent/plan roles never land on a search-only model.
 *
 * Errors are legal outcomes (e.g. "no agentic model") - only silent
 * promise violations fail. Run: node tools/route-matrix.mjs
 */
const BASE = process.env.YOAI_URL || "http://127.0.0.1:11435";

const MODES = ["offline", "online-offline", "my-hardware"];
const TASKS = ["general", "code", "reasoning", "math"];
const DIFFS = ["easy", "hard"];
const EAGS = ["balanced", "privacy", "freshness"];
const LEANS = ["balanced", "speed", "quality"];
const ROLES = [
  { agent: "0", plan: "0", name: "chat" },
  { agent: "1", plan: "0", name: "agent" },
  { agent: "1", plan: "1", name: "plan" },
];
const QUERIES = {
  neutral: "summarize this paragraph for me",
  health: "can you explain my blood test results",
  fresh: "what's the latest news about the election today",
};

let total = 0, errors = 0;
const violations = [];

for (const mode of MODES)
  for (const task of TASKS)
    for (const diff of DIFFS)
      for (const eag of EAGS)
        for (const lean of LEANS)
          for (const role of ROLES)
            for (const [qname, q] of Object.entries(QUERIES)) {
              total++;
              const params = new URLSearchParams({
                mode, task, difficulty: diff, eagerness: eag, lean, q,
                agent: role.agent, plan: role.plan,
              });
              const label = `${mode}/${task}/${diff}/${eag}/${lean}/${role.name}/${qname}`;
              let r;
              try {
                r = await (await fetch(`${BASE}/internal/route-preview?${params}`)).json();
              } catch (e) {
                violations.push(`${label}: endpoint unreachable (${e.message})`);
                continue;
              }
              if (r.error) { errors++; continue; } // legal outcome
              const online = String(r.model).startsWith("online:");
              if (online && (mode === "offline" || mode === "my-hardware"))
                violations.push(`${label}: OFFLINE PROMISE BROKEN -> ${r.model} (${r.reason})`);
              // Privacy-first may still go online for GENUINE live-web
              // needs (that's the mode's consent) - but never for hard-
              // question escalation or anything else.
              if (online && eag === "privacy" && !/current info|up-to-date/.test(r.reason))
                violations.push(`${label}: PRIVACY DIAL IGNORED -> ${r.model} (${r.reason})`);
              if (online && qname === "health" && role.name === "chat")
                violations.push(`${label}: HEALTH STAYED-HOME BROKEN -> ${r.model} (${r.reason})`);
              if (online && role.name !== "chat" && /sonar|search/.test(r.model))
                violations.push(`${label}: AGENT GOT SEARCH-ONLY MODEL -> ${r.model}`);
            }

// Size sweep: a turn far bigger than any local model holds. Promises:
//   5. offline / my-hardware modes still never resolve online, however big.
//   6. online-offline + a neutral question goes ONLINE with a "too long"
//      reason (the mode permits it; consent is the frontend's half) - unless
//      the mirrored entitlement says the user cannot use online models.
//   7. a health question stays home even when too long.
const HUGE = "200000";
for (const mode of MODES)
  for (const role of ROLES)
    for (const [qname, q] of Object.entries(QUERIES)) {
      total++;
      const params = new URLSearchParams({
        mode, task: "general", difficulty: "easy", eagerness: "balanced", lean: "balanced", q,
        agent: role.agent, plan: role.plan, turn_tokens: HUGE,
      });
      const label = `${mode}/huge-turn/${role.name}/${qname}`;
      let r;
      try {
        r = await (await fetch(`${BASE}/internal/route-preview?${params}`)).json();
      } catch (e) {
        violations.push(`${label}: endpoint unreachable (${e.message})`);
        continue;
      }
      if (r.error) { errors++; continue; }
      const online = String(r.model).startsWith("online:");
      if (online && (mode === "offline" || mode === "my-hardware"))
        violations.push(`${label}: OFFLINE PROMISE BROKEN ON SIZE -> ${r.model} (${r.reason})`);
      if (online && qname === "health" && role.name === "chat")
        violations.push(`${label}: HEALTH WENT ONLINE FOR SIZE -> ${r.model} (${r.reason})`);
      if (mode === "online-offline" && qname === "neutral" && role.name === "chat" && !online
          && process.env.YOAI_MATRIX_ENTITLED !== "0")
        violations.push(`${label}: TOO-LONG TURN STAYED LOCAL -> ${r.model} (${r.reason})`);
      if (online && mode === "online-offline" && qname === "neutral" && role.name === "chat" && !/too long/.test(r.reason))
        violations.push(`${label}: went online without the size reason -> ${r.reason}`);
    }

console.log(`swept ${total} combinations, ${errors} clean refusals`);
if (violations.length) {
  console.log(`\n${violations.length} VIOLATIONS:`);
  for (const v of violations) console.log("  " + v);
  process.exit(1);
}
console.log("all invariants hold");
