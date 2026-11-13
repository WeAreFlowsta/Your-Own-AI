// Another program holds the ports the app's engines usually take (a
// developer's own web server on 8080, a proxy in a container): the app
// leaves that program alone, moves its own server to a free port for the
// session, and the chat still answers. Before 0.9.1 it ended whatever
// listened on 8080 and, when it could not, sent every prompt there.
import net from "node:net";
import { ask, invoke, lastReply, shot, waitForChat, waitForReplyDone } from "./_helpers";

const OFFSET = Number(process.env.YOAI_PORT_OFFSET ?? 0) || 0;
const PORTS = { chat: 8080 + OFFSET, embed: 8091 + OFFSET, util: 8092 + OFFSET };

/** A listener of our own on `port`, standing in for someone else's server:
 *  it accepts connections and says nothing a model server would. */
async function hold(port: number): Promise<{ server: net.Server; connections: () => number }> {
  let seen = 0;
  for (let i = 0; i < 40; i++) {
    const server = net.createServer((socket) => {
      seen += 1;
      socket.on("error", () => undefined);
      socket.end("HTTP/1.1 418 I'm a teapot\r\ncontent-length: 0\r\nconnection: close\r\n\r\n");
    });
    const ok = await new Promise<boolean>((done) => {
      server.once("error", () => done(false));
      server.listen(port, "127.0.0.1", () => done(true));
    });
    if (ok) return { server, connections: () => seen };
    await new Promise((r) => setTimeout(r, 500));
  }
  throw new Error(`port ${port} never came free for the test's own listener`);
}

describe("another program holds the engines' usual ports", () => {
  const held: { name: string; port: number; server: net.Server; connections: () => number }[] = [];
  after(async () => {
    await Promise.all(held.map((h) => new Promise((r) => h.server.close(() => r(undefined)))));
    // Back on their usual ports for the specs that follow.
    for (const stop of ["stop_llama_server", "stop_embedding_server", "stop_utility_server"]) await invoke(stop).catch(() => undefined);
  });

  it("leaves that program running, moves its own servers, and still answers", async function () {
    this.timeout(900_000);
    await waitForChat();
    // The app's own servers stop, and "someone else" takes their ports.
    for (const stop of ["stop_llama_server", "stop_embedding_server", "stop_utility_server"]) await invoke(stop).catch(() => undefined);
    for (const [name, port] of Object.entries(PORTS)) held.push({ name, port, ...(await hold(port)) });
    console.log(`e2e: the test holds ${held.map((h) => `${h.name} ${h.port}`).join(", ")}`);

    await ask("Ports test: name one color and nothing else.");
    await waitForReplyDone(600_000);
    const reply = (await lastReply()).trim();
    console.log(`e2e: with the usual ports held the reply was: ${reply.slice(0, 120)}`);
    expect(reply.length).toBeGreaterThan(0);
    expect(reply).not.toContain("empty reply");
    await shot("ports-taken-reply");

    // Nobody's server was ended, and no prompt was sent to it.
    for (const h of held) expect(h.server.listening).toBe(true);
    const chat = held.find((h) => h.name === "chat")!;
    console.log(`e2e: connections the stand-in on the chat port saw: ${chat.connections()}`);
    expect(chat.connections()).toBeLessThanOrEqual(2); // the app's own look at who holds the port, never a prompt
  });
});
