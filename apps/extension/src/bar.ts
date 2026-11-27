import tokens from "./tokens.css" with { type: "text" };
import css from "./bar.css" with { type: "text" };
import type { PresetItem } from "./protocol";

const t = (k: string, subs?: string[]) => chrome.i18n.getMessage(k, subs);

const ICON: Record<string, string> = {
  dl: '<path d="M12 15V3M7 10l5 5 5-5M5 21h14"/>',
  chev: '<path d="m6 9 6 6 6-6"/>',
  check: '<path d="M20 6 9 17l-5-5"/>',
  alert: '<circle cx="12" cy="12" r="10"/><path d="M12 8v4M12 16h.01"/>',
  out: '<path d="M7 17 17 7M8 7h9v9"/>',
  spin: '<path d="M12 2v4M16 3.1l-2 3.5M20.9 8l-3.5 2M22 12h-4M20.9 16l-3.5-2M16 20.9l-2-3.5M12 22v-4M8 20.9l2-3.5M3.1 16l3.5-2M2 12h4M3.1 8l3.5 2M8 3.1l2 3.5"/>',
  eyeoff: '<path d="M10.7 5.1A10 10 0 0 1 12 5c7 0 10 7 10 7a13 13 0 0 1-1.7 2.7M6.6 6.6A13 13 0 0 0 2 12s3 7 10 7a10 10 0 0 0 5.4-1.6M2 2l20 20M9.9 9.9a3 3 0 0 0 4.2 4.2"/>',
};
const svg = (n: string) => `<svg class="${n === "spin" ? "spin" : ""}" viewBox="0 0 24 24" aria-hidden="true">${ICON[n]}</svg>`;

export type BarState = "checking" | "ready" | "sending" | "added" | "resting" | "duplicate" | "opened" | "starting" | "failed";

/** What the bar shows. `main` and `tag` are already localized text. */
export interface View {
  state: BarState;
  main?: string;
  tag?: string;
  presets?: PresetItem[];
  current?: string;
  detail?: string;
  inside: boolean;
}
/** Viewport coordinates of the player's top-right corner: distance from the top and from the right edge. */
export interface Anchor { top: number; right: number }

export interface Callbacks {
  main(): void;
  pick(id: string): void;
  hide(scope: "page" | "domain"): void;
  hover(on: boolean): void;
  /** Host of the top page, what "Hide on <domain>" hides. */
  host(): string;
}

const LABEL: Record<BarState, { key: string; icon: string }> = {
  checking: { key: "bar_checking", icon: "spin" },
  ready: { key: "bar_label", icon: "dl" },
  sending: { key: "bar_sending", icon: "spin" },
  added: { key: "bar_added", icon: "check" },
  resting: { key: "bar_in_ytdlpd", icon: "check" },
  duplicate: { key: "bar_duplicate", icon: "check" },
  opened: { key: "bar_opened", icon: "out" },
  starting: { key: "bar_starting", icon: "spin" },
  failed: { key: "bar_failed", icon: "alert" },
};

/** The bar in a closed shadow root: the tab above the player (or a chip inside), its preset list and its own right-click menu. */
export class Bar {
  readonly host = document.createElement("ytdlpd-bar");
  private root = this.host.attachShadow({ mode: "closed" });
  private bar = document.createElement("div");
  private main = document.createElement("button");
  private more = document.createElement("button");
  private menu = document.createElement("div");
  private ctx = document.createElement("div");
  private live = document.createElement("div");
  private view?: View;
  private anchor?: Anchor;

  constructor(private cb: Callbacks) {
    const style = document.createElement("style");
    style.textContent = tokens + "\n" + css;
    this.bar.className = "bar";
    this.main.type = "button";
    this.main.className = "main";
    this.more.type = "button";
    this.more.className = "more";
    this.more.setAttribute("aria-haspopup", "menu");
    this.more.setAttribute("aria-expanded", "false");
    this.more.setAttribute("aria-label", t("bar_more"));
    this.more.innerHTML = svg("chev");
    this.menu.className = "menu";
    this.menu.setAttribute("role", "menu");
    this.menu.setAttribute("aria-label", t("bar_presets"));
    this.ctx.className = "menu";
    this.ctx.setAttribute("role", "menu");
    this.live.className = "live";
    this.live.setAttribute("aria-live", "polite");
    this.bar.append(this.main, this.more);
    this.root.append(style, this.bar, this.menu, this.ctx, this.live);

    // a page script cannot reach this shadow root, but a synthetic click must still not count
    this.main.addEventListener("click", (e) => { if (e.isTrusted && this.main.getAttribute("aria-disabled") !== "true") { e.stopPropagation(); cb.main(); } });
    this.more.addEventListener("click", (e) => { if (e.isTrusted) { e.stopPropagation(); this.toggleMenu(); } });
    for (const ev of ["pointerdown", "mousedown", "mouseup", "dblclick"]) this.bar.addEventListener(ev, (e) => e.stopPropagation());
    this.bar.addEventListener("pointerenter", () => cb.hover(true));
    this.bar.addEventListener("pointerleave", () => cb.hover(false));
    this.bar.addEventListener("contextmenu", (e) => { e.preventDefault(); e.stopPropagation(); if (e.isTrusted) this.openCtx(); });
    this.root.addEventListener("keydown", (e) => this.key(e as KeyboardEvent));
    this.root.addEventListener("focusout", (e) => {
      const next = (e as FocusEvent).relatedTarget as Node | null;
      if (!next || !this.root.contains(next)) this.closeAll();
    });
  }

  get visible(): boolean { return this.bar.classList.contains("vis"); }

  get open(): boolean { return this.menu.classList.contains("show") || this.ctx.classList.contains("show"); }

  render(view: View, anchor: Anchor) {
    this.view = view;
    this.anchor = anchor;
    const l = LABEL[view.state];
    const text = view.state === "ready" ? view.main ?? t(l.key) : t(l.key);
    this.main.innerHTML = `${svg(l.icon)}<span>${esc(text)}</span>${view.tag ? `<span class="tag">${esc(view.tag)}</span>` : ""}`;
    const idle = view.state === "ready" || view.state === "failed";
    this.main.setAttribute("aria-disabled", String(!idle));
    this.main.setAttribute("aria-label", view.state === "ready" ? `${text}${view.tag ? " " + view.tag : ""}: ${t("bar_tip")}` : view.state === "failed" ? `${t("bar_failed")}: ${view.detail ?? ""}` : text);
    this.main.title = view.state === "failed" ? view.detail ?? "" : view.state === "ready" ? t("bar_tip") : "";
    const multi = view.state === "ready" && (view.presets?.length ?? 0) > 1;
    this.bar.className = `bar show vis${view.inside ? " inside" : ""}${multi ? "" : " single"} s-${view.state}`;
    this.live.textContent = view.state === "ready" || view.state === "checking" ? "" : text;
    this.place();
    if (this.menu.classList.contains("show")) this.fillMenu();
  }

  /** Re-position after scroll/resize without re-rendering. */
  move(anchor: Anchor) { this.anchor = anchor; this.place(); }

  private place() {
    if (!this.anchor || !this.view) return;
    const h = this.view.inside ? 28 : 24;
    // inside: 8px in from the player's top-right; tab: flush above its top edge
    const inset = this.view.inside ? 8 : 0;
    const top = this.anchor.top + (this.view.inside ? inset : -h);
    const right = this.anchor.right + inset;
    this.bar.style.top = `${Math.max(0, top)}px`;
    this.bar.style.right = `${right}px`;
    for (const m of [this.menu, this.ctx]) {
      m.style.top = `${Math.max(0, top) + h}px`;
      m.style.right = `${right}px`;
    }
  }

  hideNow() {
    this.closeAll();
    this.bar.classList.remove("show", "vis");
  }

  /** Fade out (keeps the node for the transition), unless a menu is open. */
  fade() {
    if (this.open) return;
    this.bar.classList.remove("vis");
    setTimeout(() => { if (!this.bar.classList.contains("vis")) this.bar.classList.remove("show"); }, 130);
  }

  private fillMenu() {
    const v = this.view;
    this.menu.replaceChildren();
    const h = document.createElement("div");
    h.className = "h";
    h.textContent = t("bar_presets");
    this.menu.append(h);
    for (const p of v?.presets ?? []) {
      const b = document.createElement("button");
      b.type = "button";
      b.className = "item";
      b.setAttribute("role", "menuitemradio");
      b.setAttribute("aria-checked", String(p.id === v?.current));
      b.innerHTML = `<span class="c">${p.id === v?.current ? svg("check") : ""}</span><span>${esc(p.label)}</span>`;
      b.addEventListener("click", (e) => { if (e.isTrusted) { e.stopPropagation(); this.closeAll(); this.cb.pick(p.id); } });
      this.menu.append(b);
    }
  }

  private toggleMenu() {
    if (this.menu.classList.contains("show")) return this.closeAll();
    this.ctx.classList.remove("show");
    this.fillMenu();
    this.menu.classList.add("show");
    this.more.setAttribute("aria-expanded", "true");
    (this.menu.querySelector('[aria-checked="true"]') as HTMLElement | null ?? this.menu.querySelector("button"))?.focus();
  }

  private openCtx() {
    this.menu.classList.remove("show");
    this.ctx.replaceChildren();
    const host = this.cb.host();
    for (const [scope, label] of [["page", t("menu_hide_page")], ["domain", t("menu_hide_domain", [host])]] as const) {
      const b = document.createElement("button");
      b.type = "button";
      b.className = "item";
      b.setAttribute("role", "menuitem");
      b.innerHTML = `<span class="c">${svg("eyeoff")}</span><span>${esc(label)}</span>`;
      b.addEventListener("click", (e) => { if (e.isTrusted) { e.stopPropagation(); this.closeAll(); this.cb.hide(scope); } });
      this.ctx.append(b);
    }
    this.ctx.classList.add("show");
    (this.ctx.querySelector("button") as HTMLElement).focus();
  }

  closeAll() {
    this.menu.classList.remove("show");
    this.ctx.classList.remove("show");
    this.more.setAttribute("aria-expanded", "false");
  }

  private key(e: KeyboardEvent) {
    e.stopPropagation();
    const m = this.menu.classList.contains("show") ? this.menu : this.ctx.classList.contains("show") ? this.ctx : null;
    if (e.key === "Escape") { const was = !!m; this.closeAll(); if (was) this.more.focus(); return; }
    if (!m) {
      if (e.key === "ArrowDown" && e.target === this.more) { e.preventDefault(); this.toggleMenu(); }
      return;
    }
    const items = [...m.querySelectorAll<HTMLElement>("button")];
    const at = items.indexOf(this.root.activeElement as HTMLElement);
    if (e.key === "ArrowDown") { e.preventDefault(); items[(at + 1) % items.length]?.focus(); }
    if (e.key === "ArrowUp") { e.preventDefault(); items[(at - 1 + items.length) % items.length]?.focus(); }
  }

  /** Rect of the bar, so the page can tell pointer-on-bar from pointer-off. */
  contains(node: EventTarget | null): boolean { return node === this.host; }
}

const esc = (s: string) => s.replace(/[&<>"]/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;" })[c]!);
