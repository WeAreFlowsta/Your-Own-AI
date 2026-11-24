#!/usr/bin/env node
/**
 * Build check for the claim "an online AI is shown only what was found
 * for the question you asked it, and you can turn that off".
 *
 * Memory text reaches a model's request through one builder
 * (`memory_prompt::for_turn`), which applies the AI's switch and records
 * what was sent. This check fails when memory text is built or placed
 * anywhere else, or when the builder stops asking the switch.
 */
import { readFileSync, readdirSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const SRC = resolve(dirname(fileURLToPath(import.meta.url)), "..", "src-tauri", "src");

// Function called -> the only files that may call it.
const ALLOWED = {
  "found_beside(": ["memory_prompt.rs"],
  "kept_about_them(": ["memory_prompt.rs"],
  "about_them_text(": ["memory_prompt.rs"],
  "for_turn(": ["memory_prompt.rs", "inference_server.rs"],
  "turn_text(": ["memory_block.rs", "memory_session.rs", "inference_server.rs"],
  "state_note(": ["memory_block.rs", "memory_state.rs"],
};

const problems = [];
for (const name of readdirSync(SRC)) {
  if (!name.endsWith(".rs")) continue;
  const lines = readFileSync(join(SRC, name), "utf8").split(/\r?\n/);
  lines.forEach((line, i) => {
    const code = line.replace(/\/\/.*$/, "");
    for (const [call, files] of Object.entries(ALLOWED)) {
      if (!code.includes(call) || files.includes(name)) continue;
      // A longer name that only ends the same way is another function.
      const at = code.indexOf(call);
      if (at > 0 && /[A-Za-z0-9_]/.test(code[at - 1])) continue;
      problems.push(`${name}:${i + 1} calls ${call.slice(0, -1)}; memory text is built in ${files.join(", ")} only`);
    }
  });
}

const builder = readFileSync(join(SRC, "memory_prompt.rs"), "utf8");
const asks = (builder.match(/shares_memory\(input\.online, input\.memory_shared\)/g) ?? []).length;
if (asks < 2) problems.push("memory_prompt.rs: the builder must ask the AI's switch before the search and before the kept lines about the person");
if (!builder.includes('rec.more.insert("sent"')) problems.push("memory_prompt.rs: the record of an online turn must list what was sent");

if (problems.length) {
  console.error(`check-memory-online: FAILED\n  ${problems.join("\n  ")}`);
  process.exit(1);
}
console.log("check-memory-online: ok (memory text enters a request through the one builder, which asks the AI's switch)");
