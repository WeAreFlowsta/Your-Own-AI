// Catalog URL sweep: every download URL in the offline catalog must answer.
// Publishers rename files (unsloth's Dynamic 3.0 requant broke the flagship
// download in shipped 0.4.1, silently) - this turns the next rename into a
// morning notification instead of a field incident.
import { readFileSync } from "node:fs";

const src = readFileSync(new URL("../src/data/recommended-models.ts", import.meta.url), "utf8");
const urls = [...new Set(src.match(/https:\/\/huggingface\.co\/[^'"\s]+\.gguf/g) ?? [])];
console.log(`checking ${urls.length} catalog URLs`);

const bad = [];
for (const u of urls) {
  try {
    const r = await fetch(u, { method: "HEAD", redirect: "follow" });
    if (!r.ok) bad.push(`${r.status}  ${u}`);
  } catch (e) {
    bad.push(`ERR  ${u}  (${e.message})`);
  }
}
if (bad.length) {
  console.log(`\n${bad.length} broken:\n` + bad.join("\n"));
  process.exit(1);
}
console.log("all catalog URLs answer 200");
