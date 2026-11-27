import { Client, type Store } from "./client";
import { hide } from "./hidden";
import { makeHandler } from "./worker";
import type { Msg, State } from "./protocol";

// tokens live here: keep them out of content scripts (default access includes them)
void chrome.storage.local.setAccessLevel({ accessLevel: "TRUSTED_CONTEXTS" });

const store: Store = {
  get: async (k) => (await chrome.storage.local.get(k))[k],
  set: (k, v) => chrome.storage.local.set({ [k]: v }),
};

// tokens.css --st-warn; a service worker has no CSS to read it from
const BADGE = "#f59e0b";

async function paintBadge(state: State) {
  const needsPairing = state === "unpaired" || state === "removed";
  await chrome.action.setBadgeText({ text: needsPairing ? "!" : "" });
  if (needsPairing) await chrome.action.setBadgeBackgroundColor({ color: BADGE });
  await chrome.action.setTitle({ title: chrome.i18n.getMessage(state === "offline" ? "tip_offline" : needsPairing ? "tip_unpaired" : "tip_ready") });
}

const platform = (navigator as any).userAgentData?.platform ?? "PC";
const client = new Client({
  store,
  ext: chrome.runtime.getManifest().version,
  device: () => chrome.i18n.getMessage("device_name", [platform]),
  onState: (s) => void paintBadge(s),
  // a refused loopback connect costs nothing: retry often so an app restart is picked up quickly
  backoffMax: 5000,
});

const tops = new Map<number, string>();
const handle = makeHandler({
  client,
  extId: chrome.runtime.id,
  tops,
  store,
  askTop: async (id) => { try { return (await chrome.tabs.sendMessage(id, { t: "where" }, { frameId: 0 }))?.url; } catch { return undefined; } },
  setMenuHost: (host) => void chrome.contextMenus.update("hide-domain", { title: chrome.i18n.getMessage("menu_hide_domain", [host]) }).catch(() => {}),
});

chrome.runtime.onMessage.addListener((msg: Msg, sender: any, reply: (r: unknown) => void) => {
  void handle(msg, sender).then(reply);
  return true;
});
chrome.tabs.onRemoved.addListener((id: number) => tops.delete(id));

// content scripts hold no storage: tell every frame to ask again when the switch or the hidden list changes
chrome.storage.onChanged.addListener(async (c: Record<string, unknown>) => {
  if (!c.enabled && !c.hidden) return;
  for (const tab of await chrome.tabs.query({})) if (tab.id !== undefined) chrome.tabs.sendMessage(tab.id, { t: "refresh" }).catch(() => {});
});

// the service worker sleeps after ~30 s idle: reconnect on wake and every 30 s while the app is away
chrome.alarms.create("reconnect", { periodInMinutes: 0.5 });
chrome.alarms.onAlarm.addListener((a: { name: string }) => {
  if (a.name === "reconnect" && client.state !== "ready") void client.connect();
});

function buildMenus() {
  chrome.contextMenus.removeAll(() => {
    const contexts = ["page", "video", "frame"];
    chrome.contextMenus.create({ id: "ytdlpd", title: chrome.i18n.getMessage("ext_name"), contexts });
    chrome.contextMenus.create({ id: "hide-page", parentId: "ytdlpd", title: chrome.i18n.getMessage("menu_hide_page"), contexts });
    chrome.contextMenus.create({ id: "hide-domain", parentId: "ytdlpd", title: chrome.i18n.getMessage("menu_hide_domain_generic"), contexts });
  });
}
chrome.runtime.onInstalled.addListener(buildMenus);
chrome.runtime.onStartup.addListener(buildMenus);

chrome.contextMenus.onClicked.addListener(async (info: { menuItemId: string; pageUrl?: string }) => {
  if (!info.pageUrl || (info.menuItemId !== "hide-page" && info.menuItemId !== "hide-domain")) return;
  const hidden = await store.get("hidden");
  await store.set("hidden", hide(hidden, info.menuItemId === "hide-page" ? "page" : "domain", info.pageUrl));
});

void client.connect();
