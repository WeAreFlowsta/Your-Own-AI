// Headless MCP start matrix: for each installed tool (or each directory
// listing given as JSON), start the server the way the app does and do the
// handshake: initialize -> notifications/initialized -> tools/list.
import { spawn } from "node:child_process";
import { readFileSync } from "node:fs";
import { homedir } from "node:os";
const expand = (s) => s.replace(/^~\//, homedir() + "/");
const servers = JSON.parse(readFileSync(expand("~/.your-own-ai-build/mcp-servers.json"), "utf8"));
const only = process.argv.slice(2);
const TIMEOUT = 120000;
async function probe(s) {
  if (s.transport !== "stdio") return { name: s.name, skip: "http transport" };
  if ((s.config || []).some((f) => f.required)) return { name: s.name, skip: "needs settings" };
  const cmd = expand(s.command), args = (s.args || []).map(expand);
  const t0 = Date.now();
  return new Promise((resolve) => {
    const child = spawn(cmd, args, { stdio: ["pipe", "pipe", "pipe"], env: { ...process.env, HOME: homedir() } });
    let buf = "", err = "", done = false, id = 0;
    const finish = (r) => { if (done) return; done = true; try { child.kill(); } catch {} resolve({ name: s.name, ms: Date.now() - t0, ...r, stderr: err.trim().split("\n").slice(-2).join(" | ").slice(0, 200) }); };
    const send = (m) => child.stdin.write(JSON.stringify(m) + "\n");
    const timer = setTimeout(() => finish({ error: "timeout" }), TIMEOUT);
    child.stderr.on("data", (d) => { err += d; });
    child.on("error", (e) => finish({ error: String(e.message) }));
    child.on("exit", (c) => finish({ error: `exited ${c}` }));
    child.stdout.on("data", (d) => {
      buf += d;
      let i;
      while ((i = buf.indexOf("\n")) >= 0) {
        const line = buf.slice(0, i); buf = buf.slice(i + 1);
        let msg; try { msg = JSON.parse(line); } catch { continue; }
        if (msg.id === 1) {
          send({ jsonrpc: "2.0", method: "notifications/initialized" });
          send({ jsonrpc: "2.0", id: 2, method: "tools/list", params: {} });
        } else if (msg.id === 2) {
          clearTimeout(timer);
          const tools = msg.result?.tools ?? [];
          finish({ ok: true, server: msg.result ? undefined : msg.error, tools: tools.length, sample: tools.slice(0, 4).map((t) => t.name) });
        }
      }
    });
    send({ jsonrpc: "2.0", id: 1, method: "initialize", params: { protocolVersion: "2025-06-18", capabilities: {}, clientInfo: { name: "yoai-matrix", version: "0" } } });
  });
}
for (const s of servers) {
  if (only.length && !only.includes(s.name)) continue;
  const r = await probe(s);
  console.log(JSON.stringify(r));
}
