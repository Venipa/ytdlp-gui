import type { Client } from "./client";
import { candidateUrls, isContent, isHttp, isOwn, isPopup, type Sender } from "./sender";
import { hide, hostOf, isHidden } from "./hidden";
import { WireError, type Added, type Checked, type Msg, type State } from "./protocol";

export type Reply =
  | { ok: true; supported?: boolean; checked?: Checked; added?: Added; state?: State; label?: string; port?: number; appVersion?: string; error?: string; retryAfterMs?: number; off?: boolean; host?: string }
  | { ok: false; code: string; message?: string; retryAfterMs?: number };

export interface Deps {
  client: Client;
  extId: string;
  /** Top-frame URL per tab, taken from top-frame messages (the sender URL), for the subframe fallback. */
  tops: Map<number, string>;
  setMenuHost: (host: string) => void;
  /** The worker's own storage: tokens, hidden and enabled never reach a content script. */
  store: { get(key: string): Promise<any>; set(key: string, value: any): Promise<void> };
  /** Ask a tab's top frame for its URL when `tops` has lost it (worker restart). */
  askTop?: (tabId: number) => Promise<string | undefined>;
}

const fail = (e: unknown): Reply =>
  e instanceof WireError ? { ok: false, code: e.code, message: e.message, retryAfterMs: e.retryAfterMs } : { ok: false, code: "internal" };

/** The service worker's message handler; every branch checks who sent it first. */
export function makeHandler(d: Deps) {
  const { client, extId } = d;

  /** First supported URL among candidates, else the last answer. */
  async function find(urls: string[]): Promise<{ url: string; checked: Checked }> {
    let last: { url: string; checked: Checked } | undefined;
    for (const url of urls) {
      const checked = await client.check(url);
      last = { url, checked };
      if (checked.supported) break;
    }
    if (!last) throw new WireError("bad_url");
    return last;
  }

  /** The top page of the sender's tab: the page the context menu hides, whichever frame asks. */
  async function topOf(s: Sender): Promise<string | undefined> {
    const id = s.tab?.id;
    if (s.frameId === 0 || s.frameId === undefined) return isHttp(s.url) ? s.url : undefined;
    if (id === undefined) return undefined;
    let u = d.tops.get(id) ?? (await d.askTop?.(id));
    if (isHttp(u)) d.tops.set(id, u); else u = undefined;
    return u;
  }

  return async function handle(msg: Msg, s: Sender): Promise<Reply> {
    if (!isOwn(s, extId)) return { ok: false, code: "unauthorized" };
    const popup = isPopup(s, extId);
    const content = isContent(s, extId);
    if (!popup && !content) return { ok: false, code: "unauthorized" };
    try {
      switch (msg.t) {
        case "status":
          await (client.state === "offline" ? client.connect() : Promise.resolve());
          return { ok: true, state: client.state, label: client.label, port: client.port, appVersion: client.appVersion, error: client.lastError?.code, retryAfterMs: client.lastError?.retryAfterMs };
        case "view": {
          if (!content) return { ok: false, code: "unauthorized" };
          const top = (await topOf(s)) ?? s.url!;
          const off = (await d.store.get("enabled")) === false || isHidden(await d.store.get("hidden"), top);
          return { ok: true, off, host: hostOf(top) };
        }
        case "hide": {
          if (!content || (msg.scope !== "page" && msg.scope !== "domain")) return { ok: false, code: "bad_request" };
          const top = await topOf(s);
          if (!top) return { ok: false, code: "bad_url" };
          await d.store.set("hidden", hide(await d.store.get("hidden"), msg.scope, top));
          return { ok: true };
        }
        case "top":
          if (content && s.frameId === 0 && s.tab?.id !== undefined && isHttp(s.url)) d.tops.set(s.tab.id, s.url);
          return { ok: true };
        case "host":
          const top = content ? await topOf(s) : undefined;
          if (top) d.setMenuHost(hostOf(top));
          return { ok: true };
        case "check": {
          if (!content) return { ok: false, code: "unauthorized" };
          const r = await find(candidateUrls(s, !!msg.cover, await topOf(s)));
          return { ok: true, supported: r.checked.supported, checked: r.checked };
        }
        case "add": {
          if (!content) return { ok: false, code: "unauthorized" };
          const r = await find(candidateUrls(s, !!msg.cover, await topOf(s)));
          if (!r.checked.supported) return { ok: false, code: "unsupported" };
          const preset = typeof msg.preset === "string" && r.checked.presets?.some((p) => p.id === msg.preset) ? msg.preset : null;
          return { ok: true, added: await client.add(r.url, preset) };
        }
        case "check-url": {
          if (!popup || !isHttp(msg.url)) return { ok: false, code: "bad_url" };
          const checked = await client.check(msg.url);
          return { ok: true, supported: checked.supported, checked };
        }
        case "add-url": {
          if (!popup || !isHttp(msg.url)) return { ok: false, code: "bad_url" };
          return { ok: true, added: await client.add(msg.url, null) };
        }
        case "pair":
          if (!popup) return { ok: false, code: "unauthorized" };
          await client.pair(String(msg.code));
          return { ok: true, state: client.state };
        case "disconnect":
          if (!popup) return { ok: false, code: "unauthorized" };
          await client.disconnect();
          return { ok: true, state: client.state };
        default:
          return { ok: false, code: "unknown_type" };
      }
    } catch (e) {
      return fail(e);
    }
  };
}
