/** Who sent a runtime message, and which URLs the service worker may act on. URLs always come from the browser (sender), never from the message. */

export interface Sender { id?: string; url?: string; frameId?: number; tab?: { id?: number; url?: string } }

export const isHttp = (u?: string): u is string => {
  try { const p = new URL(u ?? "").protocol; return p === "http:" || p === "https:"; } catch { return false; }
};

/** Only our own extension (content scripts or pages) may talk to the worker. */
export const isOwn = (s: Sender, extId: string): boolean => s.id === extId;

/** Our popup page: allowed to name a URL itself (it is the user's explicit "Download this page"). */
export const isPopup = (s: Sender, extId: string): boolean =>
  isOwn(s, extId) && (s.url ?? "").startsWith(`chrome-extension://${extId}/popup.html`);

/** A content script in a web page (has a tab and an http(s) frame URL). */
export const isContent = (s: Sender, extId: string): boolean =>
  isOwn(s, extId) && s.tab?.id !== undefined && isHttp(s.url);

/**
 * URLs to try for a content-script message, in order: the frame's own URL first; for a subframe the top
 * URL as well when the video covers at least half the viewport (`cover`) and the top URL is known.
 */
export function candidateUrls(s: Sender, cover: boolean, top: string | undefined): string[] {
  const out: string[] = [];
  if (isHttp(s.url)) out.push(s.url);
  const topUrl = s.tab?.url ?? top;
  if (s.frameId !== 0 && cover && isHttp(topUrl) && topUrl !== s.url) out.push(topUrl);
  return out;
}
