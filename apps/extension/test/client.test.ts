import { afterEach, describe, expect, test } from "bun:test";
import { backoff, Client, type Store } from "../src/client";

/** A fake ytdlpd bridge: speaks hello/auth/pair/check/add/ping like the real one. */
function fakeApp(o: { port?: number; instance: string; tokens?: string[]; code?: string; helloError?: { code: string; retry_after_ms?: number }; paired?: object }) {
  const tokens = new Set(o.tokens ?? []);
  const seen: string[] = [];
  const sockets = new Set<any>();
  const server = Bun.serve({
    port: o.port ?? 0,
    hostname: "127.0.0.1",
    fetch(req, s) { return s.upgrade(req) ? undefined : new Response("no", { status: 426 }); },
    websocket: {
      open: (ws) => { sockets.add(ws); },
      close: (ws) => { sockets.delete(ws); },
      message(ws, raw) {
        const m = JSON.parse(String(raw));
        seen.push(m.type);
        const reply = (body: object) => ws.send(JSON.stringify({ v: 1, id: m.id, ...body }));
        const err = (code: string) => reply({ type: "error", code, message: code });
        switch (m.type) {
          case "hello": if (o.helloError) return reply({ type: "error", message: "x", ...o.helloError }); return reply({ type: "hello_ok", app: "ytdlpd", app_version: "1.0.0", proto: 1, instance: o.instance, label: "ytdlpd" });
          case "auth": return tokens.has(m.token) ? reply({ type: "auth_ok", device: "d" }) : err("bad_token");
          case "pair": if (m.code !== o.code) return err("bad_code"); tokens.add("tok-" + o.instance); return reply(o.paired ?? { type: "paired", token: "tok-" + o.instance, device: m.device });
          case "ping": return reply({ type: "pong" });
          case "check": return reply({ type: "checked", url: m.url, supported: m.url.includes("youtube"), extractor: null, name: null, kind: "video", presets: [{ id: "video:1080", label: "Video 1080p" }], preset: "video:1080" });
          case "add": return reply({ type: "added", id: 1, state: "queued", duplicate: false });
        }
      },
    },
  });
  return { port: server.port as number, seen, tokens, sockets, stop: () => server.stop(true) };
}

function memory(init: Record<string, any> = {}): Store & { data: Record<string, any> } {
  const data = { ...init };
  return { data, get: async (k) => data[k], set: async (k, v) => { data[k] = v; } };
}

const until = async (f: () => boolean, ms = 3000) => {
  const end = Date.now() + ms;
  while (!f()) { if (Date.now() > end) throw new Error("timeout"); await Bun.sleep(5); }
};

const opts = (store: Store, ports: number[]) => ({ store, ports, ext: "0.1.0", device: () => "Chrome on Test", connectMs: 300, backoffBase: 10, backoffMax: 40, pingMs: 30 });
const closers: (() => void)[] = [];
afterEach(() => { while (closers.length) closers.pop()!(); });

describe("client", () => {
  test("backoff doubles to the cap", () => {
    expect([0, 1, 2, 3, 4, 5, 6].map((n) => backoff(n))).toEqual([1000, 2000, 4000, 8000, 16000, 30000, 30000]);
  });

  test("discovery skips dead ports and non-ytdlpd listeners", async () => {
    const app = fakeApp({ instance: "a", tokens: ["t"] });
    closers.push(app.stop);
    const store = memory({ tokens: { a: "t" } });
    const c = new Client(opts(store, [1, app.port]));
    closers.push(() => c.close());
    expect(await c.connect()).toBe("ready");
    expect(c.port).toBe(app.port);
    expect(store.data.port).toBe(app.port);
  });

  test("unpaired then pair stores the token under the instance", async () => {
    const app = fakeApp({ instance: "a", code: "12345678" });
    closers.push(app.stop);
    const store = memory();
    const c = new Client(opts(store, [app.port]));
    closers.push(() => c.close());
    expect(await c.connect()).toBe("unpaired");
    await expect(c.pair("00000000")).rejects.toMatchObject({ code: "bad_code" });
    await expect(c.pair("12ab")).rejects.toMatchObject({ code: "bad_code" });
    await c.pair("12345678");
    expect(c.state).toBe("ready");
    expect(store.data.tokens).toEqual({ a: "tok-a" });
    expect((await c.check("https://youtube.com/watch?v=1")).supported).toBe(true);
  });

  test("a rejected token wipes only that instance's token", async () => {
    const a = fakeApp({ instance: "a", tokens: [] });
    const b = fakeApp({ instance: "b", tokens: ["good"] });
    closers.push(a.stop, b.stop);
    const store = memory({ tokens: { a: "stale", b: "good" } });
    const c = new Client(opts(store, [a.port, b.port]));
    closers.push(() => c.close());
    expect(await c.connect()).toBe("ready");
    expect(c.instance).toBe("b");
    expect(store.data.tokens).toEqual({ b: "good" });
  });

  test("all instances rejecting leaves state removed", async () => {
    const a = fakeApp({ instance: "a" });
    closers.push(a.stop);
    const c = new Client(opts(memory({ tokens: { a: "stale" } }), [a.port]));
    closers.push(() => c.close());
    expect(await c.connect()).toBe("removed");
  });

  test("close 4403 forgets the token and reports removed", async () => {
    const a = fakeApp({ instance: "a", tokens: ["t"] });
    closers.push(a.stop);
    const store = memory({ tokens: { a: "t" } });
    const c = new Client(opts(store, [a.port]));
    closers.push(() => c.close());
    await c.connect();
    for (const ws of a.sockets) ws.close(4403);
    await until(() => c.state === "removed");
    expect(store.data.tokens).toEqual({});
  });

  test("reconnects with backoff after the app goes away and comes back", async () => {
    let a = fakeApp({ instance: "a", tokens: ["t"] });
    const port = a.port;
    const store = memory({ tokens: { a: "t" } });
    const c = new Client(opts(store, [port]));
    closers.push(() => c.close(), () => a.stop());
    await c.connect();
    a.stop();
    await until(() => c.state === "offline");
    a = fakeApp({ instance: "a", port, tokens: ["t"] });
    await until(() => c.state === "ready");
  });

  test("pings keep the socket busy", async () => {
    const a = fakeApp({ instance: "a", tokens: ["t"] });
    closers.push(a.stop);
    const c = new Client(opts(memory({ tokens: { a: "t" } }), [a.port]));
    closers.push(() => c.close());
    await c.connect();
    await until(() => a.seen.filter((t) => t === "ping").length >= 2);
  });

  test("check cache: one request per URL, negatives included, expires", async () => {
    const a = fakeApp({ instance: "a", tokens: ["t"] });
    closers.push(a.stop);
    let now = 0;
    const c = new Client({ ...opts(memory({ tokens: { a: "t" } }), [a.port]), now: () => now, pingMs: 60000 });
    closers.push(() => c.close());
    await c.connect();
    const n = () => a.seen.filter((t) => t === "check").length;
    await Promise.all([c.check("https://example.com/x"), c.check("https://example.com/x")]);
    expect(n()).toBe(1);
    expect((await c.check("https://example.com/x")).supported).toBe(false);
    expect(n()).toBe(1);
    now = 10 * 60 * 1000 + 1;
    await c.check("https://example.com/x");
    expect(n()).toBe(2);
  });

  test("offline and not paired surface as error codes", async () => {
    const c = new Client(opts(memory(), [1]));
    closers.push(() => c.close());
    await expect(c.check("https://youtube.com/x")).rejects.toMatchObject({ code: "offline" });
    const a = fakeApp({ instance: "a" });
    closers.push(a.stop);
    const d = new Client(opts(memory(), [a.port]));
    closers.push(() => d.close());
    await expect(d.add("https://youtube.com/x", null)).rejects.toMatchObject({ code: "not_paired" });
  });

  test("a version error on hello is surfaced, not shown as offline", async () => {
    const a = fakeApp({ instance: "a", helloError: { code: "version" } });
    closers.push(a.stop);
    const c = new Client(opts(memory(), [a.port]));
    closers.push(() => c.close());
    expect(await c.connect()).toBe("offline");
    expect(c.lastError).toEqual({ code: "version", retryAfterMs: undefined });
  });

  test("rate_limited keeps retry_after_ms and delays the next attempt", async () => {
    const a = fakeApp({ instance: "a", helloError: { code: "rate_limited", retry_after_ms: 400 } });
    closers.push(a.stop);
    const c = new Client(opts(memory(), [a.port]));
    closers.push(() => c.close());
    await c.connect();
    expect(c.lastError).toEqual({ code: "rate_limited", retryAfterMs: 400 });
    await Bun.sleep(150);
    expect(a.seen.filter((t) => t === "hello").length).toBe(1); // backoff alone would be 10 ms
  });

  test("a malformed paired reply stores no token", async () => {
    const a = fakeApp({ instance: "a", code: "12345678", paired: { type: "pong" } });
    closers.push(a.stop);
    const store = memory();
    const c = new Client(opts(store, [a.port]));
    closers.push(() => c.close());
    await c.connect();
    await expect(c.pair("12345678")).rejects.toMatchObject({ code: "internal" });
    expect(store.data.tokens).toBeUndefined();
  });

  test("the removed notice survives a worker restart", async () => {
    const a = fakeApp({ instance: "a", tokens: ["t"] });
    closers.push(a.stop);
    const store = memory({ tokens: { a: "t" } });
    const c = new Client(opts(store, [a.port]));
    closers.push(() => c.close());
    await c.connect();
    for (const ws of a.sockets) ws.close(4403);
    await until(() => c.state === "removed");
    c.close();
    const fresh = new Client(opts(store, [a.port]));
    closers.push(() => fresh.close());
    expect(await fresh.connect()).toBe("removed");
  });
});
