/** URL fix-ups for sites whose links only become real once the page has loaded them. */
export interface Rule {
  /** Hostnames the rule applies to (subdomains included). */
  hosts: string[];
  /** Matched against the URL path. */
  path: RegExp;
  /** `String.replace` replacement for the matched path part. */
  to: string;
}

export const RULES: Rule[] = [
  // beeg.com thumbnails link to /-0<id>; the page fixes it to /-<id> only after loading
  { hosts: ["beeg.com"], path: /^\/-0(\d{13,15})(?!\d)/, to: "/-$1" },
];

/** `url` with the first matching rule applied; anything unparseable or unmatched comes back as is. */
export function fixUrl(url: string, rules: Rule[] = RULES): string {
  let u: URL;
  try { u = new URL(url); } catch { return url; }
  const host = u.hostname.toLowerCase();
  for (const r of rules) {
    if (!r.hosts.some((h) => host === h || host.endsWith("." + h))) continue;
    if (!r.path.test(u.pathname)) continue;
    u.pathname = u.pathname.replace(r.path, r.to);
    return u.toString();
  }
  return url;
}
