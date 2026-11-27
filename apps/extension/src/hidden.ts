import type { Hidden } from "./protocol";

export const EMPTY: Hidden = { pages: [], domains: [] };

/** The key of a page: origin, path and query, no hash (a SPA hash change is the same page). */
export function pageKey(url: string): string {
  const u = new URL(url);
  return u.origin + u.pathname + u.search;
}

export function hostOf(url: string): string {
  return new URL(url).hostname;
}

/** A domain entry hides that host and its subdomains. */
export function isHidden(h: Hidden | undefined, url: string): boolean {
  if (!h) return false;
  let host: string, key: string;
  try { host = hostOf(url); key = pageKey(url); } catch { return false; }
  return h.pages.includes(key) || h.domains.some((d) => host === d || host.endsWith("." + d));
}

export function hide(h: Hidden | undefined, scope: "page" | "domain", url: string): Hidden {
  const cur = h ?? EMPTY;
  if (scope === "page") {
    const k = pageKey(url);
    return { ...cur, pages: cur.pages.includes(k) ? cur.pages : [...cur.pages, k] };
  }
  const d = hostOf(url);
  return { ...cur, domains: cur.domains.includes(d) ? cur.domains : [...cur.domains, d] };
}

/** Show again: lifts the page entry and every domain entry that covers the url. */
export function show(h: Hidden | undefined, url: string): Hidden {
  const cur = h ?? EMPTY;
  const host = hostOf(url);
  const k = pageKey(url);
  return {
    pages: cur.pages.filter((p) => p !== k),
    domains: cur.domains.filter((d) => !(host === d || host.endsWith("." + d))),
  };
}

/** Which scope hides this url, for the popup wording. */
export function hiddenBy(h: Hidden | undefined, url: string): "page" | "domain" | null {
  if (!h) return null;
  const host = hostOf(url);
  if (h.domains.some((d) => host === d || host.endsWith("." + d))) return "domain";
  return h.pages.includes(pageKey(url)) ? "page" : null;
}
