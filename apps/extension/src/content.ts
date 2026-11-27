import { Bar, type Anchor, type BarState, type View } from "./bar";
import type { Checked } from "./protocol";

const MIN_W = 240, MIN_H = 135, SMALL_LOOP_W = 480, BAR_H = 24, HIDE_MS = 1200, CHECKING_MS = 250, RECHECK_MS = 5000, DONE_MS = 2000, RETRY_MS = 2000;
const t = (k: string) => chrome.i18n.getMessage(k);

interface Session {
  video: HTMLVideoElement;
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
let sessions = new WeakMap<HTMLVideoElement, Session>();
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

function eligible(v: HTMLVideoElement): boolean {
  const r = v.getBoundingClientRect();
  if (r.width < MIN_W || r.height < MIN_H) return false;
  const vis = Math.max(0, Math.min(r.right, innerWidth) - Math.max(r.left, 0)) * Math.max(0, Math.min(r.bottom, innerHeight) - Math.max(r.top, 0));
  if (vis < 0.5 * r.width * r.height) return false;
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

function anchorOf(v: HTMLVideoElement): { anchor: Anchor; inside: boolean } {
  const r = v.getBoundingClientRect();
  const fs = !!document.fullscreenElement;
  const top = Math.max(0, r.top);
  // position:fixed measures from the viewport without the scrollbar
  const vw = fs ? innerWidth : document.documentElement.clientWidth;
  return { anchor: { top, right: Math.max(0, vw - Math.min(r.right, vw)) }, inside: fs || r.top < BAR_H };
}

function paint() {
  const s = active;
  if (!s || off() || s.state === "none" || !eligible(s.video)) return bar.hideNow();
  const where = anchorOf(s.video);
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
  try { r = await chrome.runtime.sendMessage({ t: "check", cover: s.cover }); } catch { r = { ok: false }; }
  clearTimeout(shown);
  if (r?.ok && r.supported) { s.checked = r.checked; s.state = "ready"; } else { s.checked = undefined; s.state = "none"; }
  if (active === s) paint();
}

function activate(v: HTMLVideoElement) {
  if (off() || !eligible(v)) return;
  clearTimeout(hideTimer);
  let s = sessions.get(v);
  if (!s) {
    s = { video: v, state: "none", askedAt: 0, cover: v.getBoundingClientRect().width * v.getBoundingClientRect().height >= 0.5 * innerWidth * innerHeight };
    sessions.set(v, s);
  }
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
  try { r = await chrome.runtime.sendMessage({ t: "add", preset, cover: s.cover }); } catch { r = { ok: false, code: "offline" }; }
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
    const v = document.elementsFromPoint(x, y).find((n): n is HTMLVideoElement => n instanceof HTMLVideoElement);
    if (v) activate(v);
    else if (!overBar && active) scheduleHide();
  });
}
function reposition() {
  if (!active || off()) return;
  const s = active;
  if (!eligible(s.video)) return bar.hideNow();
  if (!bar.visible) return; // scrolling must not bring a faded bar back
  if (bar.open) bar.move(anchorOf(s.video).anchor); // a re-render would rebuild the open menu and drop its focus
  else paint();
}

/** Ask the worker whether the bar is off here (switch, hidden page or domain of the TOP page) and the top host. */
async function refresh() {
  const r: any = await chrome.runtime.sendMessage({ t: "view" }).catch(() => undefined);
  disabled = r?.ok ? !!r.off : true;
  if (r?.ok && typeof r.host === "string") topHost = r.host;
  if (off()) bar.hideNow();
}

function nav() {
  if (location.href === lastHref) return;
  lastHref = location.href;
  sessions = new WeakMap();
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
    if (v) activate(v);
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
