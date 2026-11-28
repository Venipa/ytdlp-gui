import { Bar, type Anchor, type BarState, type View } from "./bar";
import { linkTarget, MAX_URL, RICH_H, RICH_W } from "./link";
import type { Checked } from "./protocol";

const MIN_W = RICH_W, MIN_H = RICH_H, SMALL_LOOP_W = 480, BAR_H = 24, HIDE_MS = 1200, CHECKING_MS = 250, RECHECK_MS = 5000, DONE_MS = 2000, RETRY_MS = 2000, LINK_DWELL_MS = 150;
const t = (k: string) => chrome.i18n.getMessage(k);

interface Session {
  /** A <video>, or an <a href> (a link, `url` is its resolved href when the session began). */
  el: HTMLVideoElement | HTMLAnchorElement;
  url?: string;
  checked?: Checked;
  state: BarState | "none";
  askedAt: number;
  detail?: string;
  cover: boolean;
}

// switch and hidden list live in the worker (a content script never reads storage, tokens sit there too)
let disabled = true;
let topHost = location.hostname;
let active: Session | undefined;
let linksOn = true;
let sessions = new WeakMap<Element, Session>();
const thumbs = new WeakMap<Element, Element | null>();
let linkTimer: ReturnType<typeof setTimeout> | undefined;
let linkCand: Element | undefined;
let hideTimer: ReturnType<typeof setTimeout> | undefined;
let overBar = false;
let lastHref = location.href;

const off = () => disabled;

const bar = new Bar({
  main: () => void send(active?.checked?.preset ?? null),
  pick: (id) => void send(id),
  hide: (scope) => void chrome.runtime.sendMessage({ t: "hide", scope }).catch(() => {}),
  host: () => topHost,
  hover: (on) => {
    overBar = on;
    if (on) clearTimeout(hideTimer);
    else scheduleHide();
  },
});

const onScreen = (r: DOMRect) =>
  Math.max(0, Math.min(r.right, innerWidth) - Math.max(r.left, 0)) * Math.max(0, Math.min(r.bottom, innerHeight) - Math.max(r.top, 0)) >= 0.5 * r.width * r.height;

function eligible(v: HTMLVideoElement): boolean {
  const r = v.getBoundingClientRect();
  if (r.width < MIN_W || r.height < MIN_H) return false;
  if (!onScreen(r)) return false;
  const cs = getComputedStyle(v);
  if (cs.display === "none" || cs.visibility === "hidden" || +cs.opacity === 0) return false;
  // muted autoplay loops without controls are thumbnail hover previews
  if (r.width < SMALL_LOOP_W && v.muted && !v.controls && (v.loop || v.autoplay)) return false;
  return true;
}

function fullscreenParent(): Element | null {
  const fs = document.fullscreenElement;
  return fs && !(fs instanceof HTMLVideoElement) ? fs : null; // a fullscreen <video> cannot hold children
}

/** The biggest image, video or background-image inside a link (a handful of descendants are looked at). */
function thumbOf(a: HTMLAnchorElement): Element | null {
  const hit = thumbs.get(a);
  if (hit?.isConnected) return hit;
  let best: Element | null = null, area = 0;
  for (const n of [a, ...[...a.querySelectorAll("*")].slice(0, 40)]) {
    const r = n.getBoundingClientRect();
    if (r.width * r.height <= area) continue;
    if (n instanceof HTMLImageElement || n instanceof HTMLVideoElement || n instanceof HTMLCanvasElement || getComputedStyle(n).backgroundImage !== "none") { best = n; area = r.width * r.height; }
  }
  if (best === a) best = null; // the link's own background is the link
  thumbs.set(a, best);
  return best;
}

/** Where the bar goes for this element and which variant, or null when it should not show now. */
function targetOf(el: HTMLVideoElement | HTMLAnchorElement): { rect: DOMRect; compact: boolean } | null {
  if (el instanceof HTMLVideoElement) return eligible(el) ? { rect: el.getBoundingClientRect(), compact: false } : null;
  const th = thumbOf(el);
  const link = el.getBoundingClientRect();
  const t = linkTarget(el.href, location.href, link, th?.getBoundingClientRect());
  if (!t) return null;
  const rect = t.thumb ? th!.getBoundingClientRect() : link;
  if (!onScreen(rect)) return null;
  const cs = getComputedStyle(el);
  return cs.display === "none" || cs.visibility === "hidden" ? null : { rect, compact: t.kind === "small" };
}

function anchorOf(r: DOMRect, compact: boolean): { anchor: Anchor; inside: boolean } {
  const fs = !!document.fullscreenElement;
  const top = Math.max(0, r.top);
  // position:fixed measures from the viewport without the scrollbar
  const vw = fs ? innerWidth : document.documentElement.clientWidth;
  return { anchor: { top, right: Math.max(0, vw - Math.min(r.right, vw)) }, inside: compact || fs || r.top < BAR_H };
}

function paint() {
  const s = active;
  const tg = s && targetOf(s.el);
  if (!s || off() || s.state === "none" || !tg || (s.el instanceof HTMLAnchorElement && !linksOn)) return bar.hideNow();
  const where = anchorOf(tg.rect, tg.compact);
  const c = s.checked;
  const label = c?.presets?.find((p) => p.id === c.preset)?.label ?? c?.preset ?? undefined;
  const view: View = {
    state: s.state,
    main: label,
    tag: c && (c.kind === "playlist" || c.kind === "channel") ? t("bar_playlist") : undefined,
    presets: c?.presets,
    current: c?.preset,
    detail: s.detail,
    inside: where.inside,
    compact: tg.compact,
  };
  const parent = fullscreenParent() ?? document.documentElement;
  if (document.fullscreenElement && !fullscreenParent()) return bar.hideNow();
  if (bar.host.parentNode !== parent) parent.appendChild(bar.host);
  bar.render(view, where.anchor);
}

function scheduleHide() {
  clearTimeout(hideTimer);
  hideTimer = setTimeout(() => { if (!overBar && !bar.open) bar.fade(); }, HIDE_MS);
}

async function ask(s: Session) {
  s.askedAt = Date.now();
  const shown = setTimeout(() => { if (s.state === "none" && !s.checked) { s.state = "checking"; if (active === s) paint(); } }, CHECKING_MS);
  s.state = "none";
  let r: any;
  const link = s.el instanceof HTMLAnchorElement ? s.el.href : undefined;
  try { r = await chrome.runtime.sendMessage(link ? { t: "check-link", url: link } : { t: "check", cover: s.cover }); } catch { r = { ok: false }; }
  clearTimeout(shown);
  if (r?.ok && r.supported) { s.checked = r.checked; s.state = "ready"; } else { s.checked = undefined; s.state = "none"; }
  if (r?.code === "superseded") s.askedAt = 0; // a newer link check took over: ask again on the next hover
  if (active === s) paint();
}

function activate(v: HTMLVideoElement | HTMLAnchorElement) {
  if (off() || !targetOf(v) || (v instanceof HTMLAnchorElement && (!linksOn || v.href.length > MAX_URL))) return;
  clearTimeout(hideTimer);
  let s = sessions.get(v);
  if (s && s.url !== (v instanceof HTMLAnchorElement ? v.href : undefined)) s = undefined; // the link now points elsewhere
  if (!s) {
    s = { el: v, url: v instanceof HTMLAnchorElement ? v.href : undefined, state: "none", askedAt: 0, cover: v instanceof HTMLVideoElement && v.getBoundingClientRect().width * v.getBoundingClientRect().height >= 0.5 * innerWidth * innerHeight };
    sessions.set(v, s);
  }
  // the open preset list lives over the video: moving onto it must not re-render (that drops the list)
  if (active === s && bar.open) return;
  active = s;
  // ask once; a "none" answer (app away, unsupported) is asked again after a pause
  if (!s.checked && Date.now() - s.askedAt > RECHECK_MS) void ask(s);
  paint();
}

async function send(preset: string | null, retried = false) {
  const s = active;
  if (!s || !s.checked || s.state === "sending") return;
  s.state = "sending";
  paint();
  let r: any;
  try { r = await chrome.runtime.sendMessage(s.el instanceof HTMLAnchorElement ? { t: "add-link", url: s.el.href, preset } : { t: "add", preset, cover: s.cover }); } catch { r = { ok: false, code: "offline" }; }
  if (r?.ok && r.added) {
    const a = r.added;
    s.state = a.state === "intake" ? "opened" : a.duplicate ? "duplicate" : "added";
    paint();
    const rest: BarState = a.state === "intake" ? "ready" : a.duplicate ? "duplicate" : "resting";
    setTimeout(() => { s.state = rest; if (active === s) paint(); }, a.duplicate ? 0 : DONE_MS);
    return;
  }
  const code = String(r?.code ?? "internal");
  if ((code === "busy" || code === "engine_unavailable") && !retried) {
    s.state = "starting";
    paint();
    setTimeout(() => void send(preset, true), RETRY_MS);
    return;
  }
  s.state = "failed";
  s.detail = chrome.i18n.getMessage("err_" + code) || chrome.i18n.getMessage("err_internal");
  paint();
}

let raf = 0;
function onPointer(e: PointerEvent) {
  if (!e.isTrusted || raf || off()) return; // a page can dispatch fake pointer events to make the bar appear
  const { clientX: x, clientY: y } = e;
  raf = requestAnimationFrame(() => {
    raf = 0;
    const stack = document.elementsFromPoint(x, y);
    const anchors = stack.map((n) => n.closest?.("a[href]")).map((n) => (n instanceof HTMLAnchorElement ? n : undefined));
    const vi = stack.findIndex((n) => n instanceof HTMLVideoElement && eligible(n));
    // a video over (or inside) a link to another page is that link's hover preview (YouTube home), not this page's player
    const under = vi < 0 ? undefined : anchors.slice(vi).find((n) => n && linkTarget(n.href, location.href, n.getBoundingClientRect()));
    const v = under || vi < 0 ? undefined : (stack[vi] as HTMLVideoElement); // otherwise videos win over links
    const a = v ? undefined : under ?? anchors.find(Boolean);
    if (a !== linkCand) { clearTimeout(linkTimer); linkTimer = undefined; linkCand = a; }
    if (v) activate(v);
    else if (a) {
      // a link must be rested on before it is asked about (the app allows 120 checks a minute); an open session stays alive
      if (active?.el === a) activate(a);
      else if (linksOn && !linkTimer) linkTimer = setTimeout(() => { linkTimer = undefined; activate(a); }, LINK_DWELL_MS);
    } else if (!overBar && active) scheduleHide();
  });
}
function reposition() {
  if (!active || off()) return;
  const s = active;
  const tg = targetOf(s.el);
  if (!tg) return bar.hideNow();
  if (!bar.visible) return; // scrolling must not bring a faded bar back
  if (bar.open) bar.move(anchorOf(tg.rect, tg.compact).anchor); // a re-render would rebuild the open menu and drop its focus
  else paint();
}

/** Ask the worker whether the bar is off here (switch, hidden page or domain of the TOP page) and the top host. */
async function refresh() {
  const r: any = await chrome.runtime.sendMessage({ t: "view" }).catch(() => undefined);
  disabled = r?.ok ? !!r.off : true;
  linksOn = r?.ok ? r.links !== false : true;
  if (r?.ok && typeof r.host === "string") topHost = r.host;
  if (off() || (!linksOn && active?.el instanceof HTMLAnchorElement)) bar.hideNow();
}

function nav() {
  if (location.href === lastHref) return;
  lastHref = location.href;
  sessions = new WeakMap();
  clearTimeout(linkTimer);
  linkTimer = linkCand = undefined;
  active = undefined;
  bar.hideNow();
  announce();
  void refresh();
}
function announce() {
  if (window === top) void chrome.runtime.sendMessage({ t: "top" }).catch(() => {});
}

function start() {
  document.addEventListener("pointermove", onPointer, { capture: true, passive: true });
  document.addEventListener("focusin", (e) => {
    if (!e.isTrusted) return;
    const n = e.target;
    const v = n instanceof HTMLVideoElement ? n : n instanceof Element ? n.querySelector("video") ?? n.closest("video") : null;
    const a = v || !(n instanceof Element) ? null : n.closest("a[href]");
    if (v) activate(v);
    else if (a instanceof HTMLAnchorElement) activate(a);
  }, true);
  document.addEventListener("contextmenu", () => void chrome.runtime.sendMessage({ t: "host" }).catch(() => {}), true);
  document.addEventListener("fullscreenchange", () => { const was = bar.visible; bar.hideNow(); if (was) paint(); });
  addEventListener("scroll", reposition, { capture: true, passive: true });
  addEventListener("resize", reposition);
  addEventListener("popstate", nav);
  addEventListener("hashchange", nav);
  // pushState/replaceState do not fire events; the Navigation API does where it exists, a cheap poll covers the rest
  (globalThis as any).navigation?.addEventListener("navigatesuccess", () => setTimeout(nav, 150));
  setInterval(nav, 1000);
  // the popup asks the top frame where it is (the worker has no tab URL without the tabs permission)
  chrome.runtime.onMessage.addListener((m: any, s: any, reply: (r: unknown) => void) => {
    if (s.id !== chrome.runtime.id) return;
    if (m?.t === "refresh") void refresh();
    if (m?.t === "where" && window === top) reply({ url: location.href });
  });
  announce();
}

// the host is always mounted and looks the same whatever the state, so its presence tells a page nothing
document.documentElement.appendChild(bar.host);
refresh().then(start);
