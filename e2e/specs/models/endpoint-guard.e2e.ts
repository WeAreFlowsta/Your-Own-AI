// The local endpoint answers a keyless request only when it is addressed to
// this computer: by a loopback name or address, and, when a browser sent
// it, from a page served on this computer. Asked from the test's own
// process with plain HTTP, so the Host and Origin headers are exactly the
// ones written here.
import { request } from "node:http";
import { waitForChat, invoke } from "./_helpers";

function get(port: number, headers: Record<string, string>): Promise<{ status: number; body: string }> {
  return new Promise((done, fail) => {
    const req = request({ host: "127.0.0.1", port, path: "/v1/models", method: "GET", headers }, (res) => {
      let body = "";
      res.on("data", (c) => (body += c));
      res.on("end", () => done({ status: res.statusCode ?? 0, body }));
    });
    req.on("error", fail);
    req.end();
  });
}

describe("the local endpoint's guard", () => {
  it("answers programs on this computer and refuses a request addressed elsewhere", async () => {
    await waitForChat();
    const port = await invoke<number>("inference_port");
    const cases: [string, Record<string, string>, number][] = [
      ["a program, by address", { Host: `127.0.0.1:${port}` }, 200],
      ["a program, by name", { Host: `localhost:${port}` }, 200],
      ["a web interface served on this computer", { Host: `127.0.0.1:${port}`, Origin: "http://localhost:3000" }, 200],
      ["a request under another name", { Host: `example.org:${port}` }, 403],
      ["a name that only starts like this computer's", { Host: `127.0.0.1.example.org:${port}` }, 403],
      ["a page elsewhere", { Host: `127.0.0.1:${port}`, Origin: "https://example.org" }, 403],
    ];
    for (const [what, headers, want] of cases) {
      const r = await get(port, headers);
      console.log(`e2e: ${what}: ${r.status}${r.status === 403 ? ` ${r.body.slice(0, 120)}` : ""}`);
      expect(`${what}: ${r.status}`).toBe(`${what}: ${want}`);
    }
  });
});
