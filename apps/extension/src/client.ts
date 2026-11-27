import { fixUrl } from "./rules";
import { PORTS, PROTO, WireError, type Added, type Checked, type Frame, type HelloOk, type State } from "./protocol";

export interface Store { get(key: string): Promise<any>; set(key: string, value: any): Promise<void> }

export interface Options {
  store: Store;
  ext: string;
  device: () => string;
  ports?: number[];
  WS?: typeof WebSocket;
  connectMs?: number;
  requestMs?: number;
  pingMs?: number;
  backoffBase?: number;
  backoffMax?: number;
  cacheMs?: number;
  now?: () => number;
  onState?: (s: State) => void;
}

/** Delay before reconnect attempt `n` (0 based): 1 s doubling to 30 s by default. */
export function backoff(n: number, base = 1000, max = 30000): number {
  return Math.min(max, base * 2 ** n);
}

interface Pending { resolve: (f: Frame) => void; reject: (e: unknown) => void; timer: ReturnType<typeof setTimeout> }

/** The one connection to the app: discovery, auth per instance, requests, 10 min check cache. */
export class Client {
  state: State = "offline";
  instance?: string;
  label?: string;
  appVersion?: string;
  port?: number;
  /** Why the last discovery failed in a way that is not "app away" (version mismatch, rate limit). */
  lastError?: { code: string; retryAfterMs?: number };
  private ws?: WebSocket;
  private nextId = 1;
  private pending = new Map<number, Pending>();
  private connecting?: Promise<State>;
  private attempts = 0;
  private reconnect?: ReturnType<typeof setTimeout>;
  private pinger?: ReturnType<typeof setInterval>;
  private cache = new Map<string, { res: Checked; t: number }>();
  private inflight = new Map<string, Promise<Checked>>();
  private unpaired?: { port: number };
  private removed = false;
  private stopped = false;
  private o: Required<Omit<Options, "onState">> & { onState?: (s: State) => void };

  constructor(o: Options) {
    this.o = {
      ports: PORTS, WS: globalThis.WebSocket, connectMs: 200, requestMs: 5000, pingMs: 20000,
      backoffBase: 1000, backoffMax: 30000, cacheMs: 600000, now: Date.now, ...o,
    };
  }

  private set(s: State) {
    if (this.state !== s) { this.state = s; this.o.onState?.(s); }
  }

  /** Find the app and authenticate; deduped while one is running. */
  connect(): Promise<State> {
    this.connecting ??= this.discover().finally(() => { this.connecting = undefined; });
    return this.connecting;
  }

  private async tokens(): Promise<Record<string, string>> {
    return (await this.o.store.get("tokens")) ?? {};
  }

  private async open(port: number): Promise<{ ws: WebSocket; hello: HelloOk } | null> {
    const ws = new this.o.WS(`ws://127.0.0.1:${port}`);
    const dead = (): null => { try { ws.close(); } catch { /* already closed */ } return null; };
    const opened = await new Promise<boolean>((res) => {
      const t = setTimeout(() => res(false), this.o.connectMs);
      ws.onopen = () => { clearTimeout(t); res(true); };
      ws.onerror = () => { clearTimeout(t); res(false); };
      ws.onclose = () => { clearTimeout(t); res(false); };
    });
    if (!opened) return dead();
    try {
      const hello = (await this.raw(ws, { type: "hello", client: "chrome", ext: this.o.ext }, this.o.connectMs * 5)) as unknown as HelloOk;
      if (hello.app !== "ytdlpd") return dead();
      return { ws, hello };
    } catch (e) {
      if (e instanceof WireError && (e.code === "version" || e.code === "rate_limited")) this.lastError = { code: e.code, retryAfterMs: e.retryAfterMs };
      return dead();
    }
  }

  /** One request/response on a socket that is not yet adopted (id 1). */
  private raw(ws: WebSocket, body: Record<string, unknown>, ms: number): Promise<Frame> {
    return new Promise((resolve, reject) => {
      const t = setTimeout(() => { ws.removeEventListener("message", on); reject(new WireError("timeout")); }, ms);
      const on = (e: MessageEvent) => {
        let f: Frame;
        try { f = JSON.parse(String(e.data)); } catch { return; }
        if (f.id !== 1) return;
        clearTimeout(t);
        ws.removeEventListener("message", on);
        if (f.type === "error") reject(new WireError(String(f.code), String(f.message ?? f.code), f.retry_after_ms as number | undefined));
        else resolve(f);
      };
      ws.addEventListener("message", on);
      ws.send(JSON.stringify({ v: PROTO, id: 1, ...body }));
    });
  }

  private async discover(): Promise<State> {
    if (this.stopped) return this.state;
    const last = await this.o.store.get("port");
    const ports = [...new Set([...(typeof last === "number" ? [last] : []), ...this.o.ports])];
    this.unpaired = undefined;
    this.lastError = undefined;
    let removed = false;
    this.removed ||= (await this.o.store.get("removed")) === true;
    for (const port of ports) {
      const r = await this.open(port);
      if (!r) continue;
      const tokens = await this.tokens();
      const token = tokens[r.hello.instance];
      if (!token) {
        this.unpaired ??= { port };
        r.ws.close();
        continue;
      }
      try {
        await this.raw(r.ws, { type: "auth", token }, this.o.requestMs);
      } catch (e) {
        if (e instanceof WireError && e.code === "bad_token") {
          // wipe only the token this instance rejected; another instance may still know its own
          delete tokens[r.hello.instance];
          await this.o.store.set("tokens", tokens);
          removed = true;
          this.unpaired ??= { port };
        } else if (e instanceof WireError && e.code === "rate_limited") {
          this.lastError = { code: e.code, retryAfterMs: e.retryAfterMs };
        }
        r.ws.close();
        continue;
      }
      this.adopt(r.ws, r.hello, port);
      await this.o.store.set("port", port);
      return this.state;
    }
    this.removed = removed || this.removed;
    if (removed) await this.o.store.set("removed", true);
    this.set(this.unpaired ? (this.removed ? "removed" : "unpaired") : "offline");
    if (!this.unpaired) this.schedule();
    return this.state;
  }

  private adopt(ws: WebSocket, hello: HelloOk, port: number) {
    this.ws = ws;
    this.instance = hello.instance;
    this.label = hello.label;
    this.appVersion = hello.app_version;
    this.port = port;
    this.attempts = 0;
    this.removed = false;
    void this.o.store.set("removed", false);
    ws.onmessage = (e) => this.onMessage(e);
    ws.onclose = (e) => void this.onClose(ws, e.code);
    this.pinger = setInterval(() => { this.request({ type: "ping" }).catch(() => {}); }, this.o.pingMs);
    this.set("ready");
  }

  private onMessage(e: MessageEvent) {
    let f: Frame;
    try { f = JSON.parse(String(e.data)); } catch { return; }
    if (typeof f.id !== "number") return;
    const p = this.pending.get(f.id);
    if (!p) return;
    this.pending.delete(f.id);
    clearTimeout(p.timer);
    if (f.type === "error") {
      if (f.code === "rate_limited") this.lastError = { code: "rate_limited", retryAfterMs: f.retry_after_ms as number | undefined };
      p.reject(new WireError(String(f.code), String(f.message ?? f.code), f.retry_after_ms as number | undefined));
    } else p.resolve(f);
  }

  private async onClose(ws: WebSocket, code: number) {
    if (this.ws !== ws) return;
    this.ws = undefined;
    clearInterval(this.pinger);
    for (const p of this.pending.values()) { clearTimeout(p.timer); p.reject(new WireError("offline")); }
    this.pending.clear();
    if (code === 4403 && this.instance) {
      // revoked in the app: forget this instance's token only
      const t = await this.tokens();
      delete t[this.instance];
      await this.o.store.set("tokens", t);
      this.removed = true;
      await this.o.store.set("removed", true);
      this.unpaired = this.port === undefined ? undefined : { port: this.port };
      this.set("removed");
      return;
    }
    this.set("offline");
    this.schedule();
  }

  private schedule() {
    if (this.stopped || this.reconnect) return;
    // a rate limit says how long to stay away; close 4429/4401 arrive after such an error frame
    const d = Math.max(backoff(this.attempts++, this.o.backoffBase, this.o.backoffMax), this.lastError?.retryAfterMs ?? 0);
    this.reconnect = setTimeout(() => { this.reconnect = undefined; void this.connect(); }, d);
  }

  private request(body: Record<string, unknown>): Promise<Frame> {
    const ws = this.ws;
    if (!ws || ws.readyState !== 1) return Promise.reject(new WireError("offline"));
    const id = ++this.nextId;
    return new Promise((resolve, reject) => {
      const timer = setTimeout(() => { this.pending.delete(id); reject(new WireError("timeout")); }, this.o.requestMs);
      this.pending.set(id, { resolve, reject, timer });
      ws.send(JSON.stringify({ v: PROTO, id, ...body }));
    });
  }

  /** Pair with the 8-digit code on the first unpaired instance found; leaves the client connected. */
  async pair(code: string, device = this.o.device()): Promise<void> {
    if (!/^\d{8}$/.test(code)) throw new WireError("bad_code");
    if (!this.unpaired) await this.connect();
    if (this.state === "ready") return;
    const target = this.unpaired;
    if (!target) throw new WireError("offline");
    const r = await this.open(target.port);
    if (!r) throw new WireError(this.lastError?.code ?? "offline", undefined, this.lastError?.retryAfterMs);
    try {
      const f = await this.raw(r.ws, { type: "pair", code, device }, this.o.requestMs);
      if (f.type !== "paired" || typeof f.token !== "string" || !f.token) throw new WireError("internal");
      const tokens = await this.tokens();
      tokens[r.hello.instance] = f.token;
      await this.o.store.set("tokens", tokens);
      await this.o.store.set("port", target.port);
      this.adopt(r.ws, r.hello, target.port);
    } catch (e) {
      r.ws.close();
      throw e;
    }
  }

  /** Forget this instance's token and drop the connection. */
  async disconnect(): Promise<void> {
    if (this.instance) {
      const t = await this.tokens();
      delete t[this.instance];
      await this.o.store.set("tokens", t);
    }
    const ws = this.ws;
    this.unpaired = this.port === undefined ? undefined : { port: this.port };
    this.removed = false;
    await this.o.store.set("removed", false);
    if (ws) { this.ws = undefined; clearInterval(this.pinger); ws.close(); }
    this.set("unpaired");
  }

  private async ready() {
    if (this.state !== "ready") await this.connect();
    if (this.state !== "ready") throw new WireError(this.state === "offline" ? "offline" : "not_paired");
  }

  /** `supported` answer for a URL, cached 10 min including negatives; concurrent calls share one request. */
  async check(url: string): Promise<Checked> {
    url = fixUrl(url);
    const hit = this.cache.get(url);
    if (hit && this.o.now() - hit.t < this.o.cacheMs) return hit.res;
    const busy = this.inflight.get(url);
    if (busy) return busy;
    const p = (async () => {
      await this.ready();
      const res = (await this.request({ type: "check", url })) as unknown as Checked;
      this.cache.set(url, { res, t: this.o.now() });
      return res;
    })().finally(() => this.inflight.delete(url));
    this.inflight.set(url, p);
    return p;
  }

  async add(url: string, preset: string | null, start = true): Promise<Added> {
    url = fixUrl(url);
    await this.ready();
    return (await this.request({ type: "add", url, preset, start })) as unknown as Added;
  }

  /** Stop all timers and close (tests, teardown). */
  close() {
    this.stopped = true;
    clearTimeout(this.reconnect);
    clearInterval(this.pinger);
    this.ws?.close();
  }
}
