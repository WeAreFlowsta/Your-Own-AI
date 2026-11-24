#!/usr/bin/env node
/**
 * Build check: every place where a person's text leaves the conversation
 * it was written in passes through the secret mask (secret_mask.rs). Each
 * row names a file, the text that marks the exit, and the mask call that
 * must stand within a few lines of it. An exit that is renamed or moved
 * fails here until this list is updated with it, so the list stays the
 * true list of exits.
 */
import { readFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const REPO = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const EXITS = [
  { what: "what memory adds to a request, on every path", file: "src-tauri/src/memory_prompt.rs", at: "pub(crate) fn block(held: &Held)", call: "secret_mask::hand_on(", within: 4 },
  { what: "document passages given to a model", file: "src-tauri/src/corpus.rs", at: "out.push(RecallHit {", call: "secret_mask::hand_on(", within: 8 },
  { what: "the digest a resumed session starts from", file: "src/hooks/useAgentSession.ts", at: "buildResumeDigest(props.chatState.messages.slice(0, -2))", call: "maskText(", within: 0 },
  { what: "the session digest kept as memory", file: "src/hooks/useAgentSession.ts", at: "const distillMemory$ = $(async (folderPath: string, rawDigest: string) => {", call: "maskText(rawDigest)", within: 4 },
  { what: "the page's mask call fails closed", file: "src/utils/secretMask.ts", at: "export async function maskText(", call: 'return "";', within: 9 },
];
// Text a log line must not carry: the first characters of a message.
const NO_TEXT_IN_LOGS = [{ file: "src/hooks/useChat.ts", never: "last user message on the wire (" }];

const problems = [];
for (const e of EXITS) {
  const lines = readFileSync(join(REPO, e.file), "utf8").split(/\r?\n/);
  const at = lines.findIndex((l) => l.includes(e.at));
  if (at < 0) {
    problems.push(`${e.file}: the exit "${e.what}" was not found (${e.at})`);
    continue;
  }
  if (!lines.slice(at, at + e.within + 1).some((l) => l.includes(e.call))) {
    problems.push(`${e.file}:${at + 1} ${e.what}: no ${e.call} within ${e.within} line(s)`);
  }
}
for (const n of NO_TEXT_IN_LOGS) {
  if (readFileSync(join(REPO, n.file), "utf8").includes(n.never)) problems.push(`${n.file}: a log line carries message text again`);
}
if (problems.length) {
  console.error("check-mask-exits: text leaves a conversation unmasked\n  " + problems.join("\n  "));
  process.exit(1);
}
console.log(`check-mask-exits: ok (${EXITS.length} exits call the mask)`);
