import { hiddenBy, hostOf, show } from "./hidden";
import type { Hidden } from "./protocol";

const t = (k: string, subs?: string[]) => chrome.i18n.getMessage(k, subs);
const $ = <T extends HTMLElement>(id: string) => document.getElementById(id) as T;
const send = (m: unknown): Promise<any> => chrome.runtime.sendMessage(m);
const errText = (code?: string, retryMs?: number) =>
  code === "rate_limited" && retryMs ? t("err_rate_limited_wait", [String(Math.ceil(retryMs / 1000))]) : t("err_" + code) || t("err_internal");

for (const el of document.querySelectorAll<HTMLElement>("[data-i18n]")) el.textContent = t(el.dataset.i18n!);
for (const el of document.querySelectorAll<HTMLElement>("[data-i18n-label]")) el.setAttribute("aria-label", t(el.dataset.i18nLabel!));

function only(id: "offline" | "pair" | "ready") {
  for (const s of ["offline", "pair", "ready"]) $(s).hidden = s !== id;
}
function head(dot: string, text: string) {
  $("dot").className = "dot " + dot;
  $("status").textContent = text;
}

async function render() {
  const r = await send({ t: "status" }).catch(() => ({ ok: false }));
  const state = r?.ok ? r.state : "offline";
  if (state === "ready") {
    head("", t("popup_connected"));
    $("version").textContent = "v" + chrome.runtime.getManifest().version;
    only("ready");
    return readyView();
  }
  if (state === "unpaired" || state === "removed") {
    head("warn", t("popup_not_paired"));
    $("removed").hidden = state !== "removed";
    only("pair");
    $("code").focus();
    return;
  }
  head("off", t("popup_offline_short"));
  $("offline-text").textContent = r?.ok && r.error ? errText(r.error, r.retryAfterMs) : t("popup_offline");
  only("offline");
}

// ---- pairing: one hidden input drawn as 4+4 slots
const code = $<HTMLInputElement>("code");
function drawSlots() {
  const d = code.value.replace(/\D/g, "").slice(0, 8);
  const slots = document.querySelectorAll<HTMLElement>(".s");
  slots.forEach((s, i) => { s.textContent = d[i] ?? ""; s.classList.toggle("act", i === Math.min(d.length, 7)); });
  return d;
}
code.addEventListener("input", () => { code.value = drawSlots(); $("pair-err").hidden = true; });
code.addEventListener("keydown", (e) => { if (e.key === "Enter") $("connect").click(); });
$("connect").addEventListener("click", async () => {
  const d = drawSlots();
  const err = $("pair-err");
  if (d.length !== 8) { err.textContent = errText("bad_code"); err.hidden = false; return; }
  const r = await send({ t: "pair", code: d }).catch(() => ({ ok: false, code: "offline" }));
  if (r?.ok) { code.value = ""; return render(); }
  err.textContent = errText(r?.code, r?.retryAfterMs);
  err.hidden = false;
});
$("retry").addEventListener("click", render);
$("disconnect").addEventListener("click", async () => { await send({ t: "disconnect" }); render(); });

// ---- paired view: current site
async function pageUrl(): Promise<string | undefined> {
  const [tab] = await chrome.tabs.query({ active: true, currentWindow: true });
  if (tab?.id === undefined) return undefined;
  try { return (await chrome.tabs.sendMessage(tab.id, { t: "where" }, { frameId: 0 }))?.url; } catch { return undefined; }
}

async function readyView() {
  const store = await chrome.storage.local.get(["enabled", "hidden"]);
  const sw = $("enabled");
  sw.setAttribute("aria-checked", String(store.enabled !== false));
  const url = await pageUrl();
  const dl = $<HTMLButtonElement>("download");
  dl.disabled = true;
  $("show").hidden = true;
  if (!url) {
    $("host").textContent = "";
    $("site").textContent = t("popup_no_page");
    $("support").textContent = "";
    return;
  }
  const host = hostOf(url);
  $("host").textContent = host;
  const hidden = store.hidden as Hidden | undefined;
  const by = hiddenBy(hidden, url);
  $("site").textContent = by === "domain" ? t("popup_hidden_domain", [host]) : by === "page" ? t("popup_hidden_page") : "";
  $("show").hidden = !by;
  $("show").onclick = async () => { await chrome.storage.local.set({ hidden: show(hidden, url) }); readyView(); };
  const c = await send({ t: "check-url", url }).catch(() => ({ ok: false }));
  if (c?.ok && c.supported) {
    $("support").textContent = t("popup_supported", [c.checked?.name ?? ""]);
    dl.disabled = false;
    dl.onclick = async () => {
      dl.disabled = true;
      const r = await send({ t: "add-url", url }).catch(() => ({ ok: false, code: "offline" }));
      const out = $("result");
      out.hidden = false;
      out.textContent = r?.ok ? t(r.added.state === "intake" ? "bar_opened" : r.added.duplicate ? "bar_duplicate" : "bar_added") : errText(r?.code);
      dl.disabled = false;
    };
  } else {
    $("support").textContent = c?.ok ? t("popup_unsupported") : errText(c?.code);
  }
}

$("enabled").addEventListener("click", async () => {
  const on = $("enabled").getAttribute("aria-checked") !== "true";
  $("enabled").setAttribute("aria-checked", String(on));
  await chrome.storage.local.set({ enabled: on });
});

void render();
