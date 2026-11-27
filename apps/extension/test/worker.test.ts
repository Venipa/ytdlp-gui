import { describe, expect, test } from "bun:test";
import { candidateUrls, isContent, isPopup } from "../src/sender";
import { makeHandler } from "../src/worker";
import { WireError, type Checked } from "../src/protocol";

const ID = "abcdefghijklmnopabcdefghijklmnop";
const content = (url: string, frameId = 0, tabUrl?: string) => ({ id: ID, url, frameId, tab: { id: 7, url: tabUrl } });
const popup = { id: ID, url: `chrome-extension://${ID}/popup.html` };

const checked = (supported: boolean): Checked => ({ url: "", supported, extractor: null, name: "YouTube", kind: "video", presets: [{ id: "video:1080", label: "Video 1080p" }], preset: "video:1080" });

function setup(supportedUrls: string[]) {
  const calls: any[] = [];
  const client: any = {
    state: "ready",
    check: async (u: string) => { calls.push(["check", u]); return checked(supportedUrls.includes(u)); },
    add: async (u: string, p: string | null) => { calls.push(["add", u, p]); return { id: 1, state: "queued", duplicate: false }; },
    pair: async (c: string) => { calls.push(["pair", c]); },
    disconnect: async () => { calls.push(["disconnect"]); },
  };
  const tops = new Map<number, string>();
  const hosts: string[] = [];
  const data: Record<string, any> = {};
  const store = { get: async (k: string) => data[k], set: async (k: string, v: any) => { data[k] = v; } };
  return { calls, tops, hosts, data, handle: makeHandler({ client, extId: ID, tops, store, setMenuHost: (h) => hosts.push(h) }) };
}

describe("sender checks", () => {
  test("other extensions and web pages are refused", async () => {
    const { handle, calls } = setup(["https://youtube.com/watch?v=1"]);
    expect(await handle({ t: "check", cover: false }, { id: "someone-else", url: "https://youtube.com/watch?v=1", tab: { id: 1 } })).toMatchObject({ ok: false, code: "unauthorized" });
    expect(await handle({ t: "status" }, { url: "https://evil.example/" })).toMatchObject({ ok: false, code: "unauthorized" });
    expect(calls).toEqual([]);
  });

  test("pair and disconnect only from the popup", async () => {
    const { handle, calls } = setup([]);
    expect(await handle({ t: "pair", code: "12345678" }, content("https://a.example/"))).toMatchObject({ ok: false, code: "unauthorized" });
    expect(await handle({ t: "disconnect" }, content("https://a.example/"))).toMatchObject({ ok: false, code: "unauthorized" });
    expect((await handle({ t: "pair", code: "12345678" }, popup)).ok).toBe(true);
    expect(calls).toEqual([["pair", "12345678"]]);
  });

  test("popup page check needs the popup url", () => {
    expect(isPopup({ id: ID, url: `chrome-extension://${ID}/other.html` }, ID)).toBe(false);
    expect(isPopup(popup, ID)).toBe(true);
    expect(isContent({ id: ID, url: "chrome://x", tab: { id: 1 } }, ID)).toBe(false);
  });

  test("the page url comes from the sender, never from the message", async () => {
    const { handle, calls } = setup(["https://youtube.com/watch?v=1"]);
    const r = await handle({ t: "check", cover: false, url: "https://evil.example/" } as any, content("https://youtube.com/watch?v=1"));
    expect(r).toMatchObject({ ok: true, supported: true });
    expect(calls).toEqual([["check", "https://youtube.com/watch?v=1"]]);
  });

  test("add sends the supported frame URL with a known preset, unknown presets become default", async () => {
    const { handle, calls } = setup(["https://youtube.com/watch?v=1"]);
    await handle({ t: "add", preset: "video:1080", cover: false }, content("https://youtube.com/watch?v=1"));
    await handle({ t: "add", preset: "rm -rf", cover: false }, content("https://youtube.com/watch?v=1"));
    expect(calls.filter((c) => c[0] === "add")).toEqual([["add", "https://youtube.com/watch?v=1", "video:1080"], ["add", "https://youtube.com/watch?v=1", null]]);
  });

  test("add on an unsupported page is refused", async () => {
    const { handle } = setup([]);
    expect(await handle({ t: "add", preset: null, cover: false }, content("https://example.com/"))).toMatchObject({ ok: false, code: "unsupported" });
  });

  test("subframe tries its own URL then the top URL only when the video covers the page", async () => {
    const top = "https://site.example/watch";
    const s = setup([top]);
    s.tops.set(7, top);
    const frame = content("https://cdn.example/embed/1", 3);
    expect((await s.handle({ t: "check", cover: false }, frame)).ok && s.calls.length).toBe(1);
    s.calls.length = 0;
    const r = await s.handle({ t: "check", cover: true }, frame);
    expect(r).toMatchObject({ ok: true, supported: true });
    expect(s.calls).toEqual([["check", "https://cdn.example/embed/1"], ["check", top]]);
  });

  test("top URL is recorded only from top-frame content messages", async () => {
    const s = setup([]);
    await s.handle({ t: "top" }, content("https://sub.example/", 2));
    expect(s.tops.size).toBe(0);
    await s.handle({ t: "top" }, content("https://a.example/", 0));
    expect(s.tops.get(7)).toBe("https://a.example/");
  });

  test("the menu host is the top page's, whichever frame asks, never one named in the message", async () => {
    const s = setup([]);
    s.tops.set(7, "https://site.example/watch");
    await s.handle({ t: "host", host: "evil.example" } as any, content("https://cdn.example/embed", 3));
    await s.handle({ t: "host" }, content("https://youtube.com/"));
    expect(s.hosts).toEqual(["site.example", "youtube.com"]);
  });

  test("hidden and enabled are judged on the top page for every frame", async () => {
    const s = setup([]);
    s.tops.set(7, "https://site.example/watch?v=1");
    const frame = content("https://cdn.example/embed/1", 3);
    expect(await s.handle({ t: "view" }, frame)).toMatchObject({ ok: true, off: false, host: "site.example" });
    // hiding from inside the iframe hides the top page, not the iframe URL
    await s.handle({ t: "hide", scope: "page" }, frame);
    expect(s.data.hidden).toEqual({ pages: ["https://site.example/watch?v=1"], domains: [] });
    expect(await s.handle({ t: "view" }, frame)).toMatchObject({ off: true });
    expect(await s.handle({ t: "view" }, content("https://site.example/watch?v=1"))).toMatchObject({ off: true });
    s.data.hidden = { pages: [], domains: ["site.example"] };
    expect(await s.handle({ t: "view" }, frame)).toMatchObject({ off: true });
    s.data.hidden = undefined;
    s.data.enabled = false;
    expect(await s.handle({ t: "view" }, frame)).toMatchObject({ off: true });
  });

  test("view and hide are for content scripts only, scope is checked", async () => {
    const s = setup([]);
    expect(await s.handle({ t: "view" }, popup)).toMatchObject({ ok: false, code: "unauthorized" });
    expect(await s.handle({ t: "hide", scope: "all" as any }, content("https://a.example/"))).toMatchObject({ ok: false });
    expect(s.data.hidden).toBeUndefined();
  });

  test("a lost top URL is asked from the top frame", async () => {
    const client: any = { state: "ready" };
    const asked: number[] = [];
    const handle = makeHandler({ client, extId: ID, tops: new Map(), store: { get: async () => undefined, set: async () => {} }, askTop: async (id) => { asked.push(id); return "https://top.example/"; }, setMenuHost: () => {} });
    expect(await handle({ t: "view" }, content("https://cdn.example/e", 2))).toMatchObject({ host: "top.example" });
    expect(asked).toEqual([7]);
  });

  test("wire errors become reply codes", async () => {
    const calls: any[] = [];
    const client: any = { state: "ready", check: async () => { throw new WireError("busy"); } };
    const handle = makeHandler({ client, extId: ID, tops: new Map(), store: { get: async () => undefined, set: async () => {} }, setMenuHost: () => {} });
    expect(await handle({ t: "check", cover: false }, content("https://a.example/"))).toMatchObject({ ok: false, code: "busy" });
    expect(calls).toEqual([]);
  });

  test("candidateUrls skips non-http frames", () => {
    expect(candidateUrls({ url: "about:blank", frameId: 1 }, true, "https://a.example/")).toEqual(["https://a.example/"]);
  });
});
