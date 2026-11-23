// Fetches GitHub repo and release stats at build time into data/stats.json.
// The page reads only that JSON; no API calls at runtime.
// Usage: bun scripts/fetch-stats.ts  (env: GITHUB_TOKEN optional; SITE_REPO, see site.config.mjs)
import { writeFileSync } from 'node:fs';
import { join } from 'node:path';
import { minMajor, repo } from '../site.config.mjs';

export type Asset = { name: string; size: number; download_count: number; url: string; sha256: string | null };
export type Stats = {
  repo: string;
  fetched_at: string;
  stars: number | null;
  forks: number | null;
  latest: { tag: string; name: string; published_at: string; url: string; assets: Asset[] } | null;
  total_downloads: number;
};

const out = join(import.meta.dir, '..', 'data', 'stats.json');
const headers: Record<string, string> = {
  accept: 'application/vnd.github+json',
  'x-github-api-version': '2022-11-28',
  'user-agent': 'ytdlpd-website-build',
};
if (process.env.GITHUB_TOKEN) headers.authorization = `Bearer ${process.env.GITHUB_TOKEN}`;

async function api<T>(path: string): Promise<T | null> {
  const res = await fetch(`https://api.github.com/repos/${repo}${path}`, { headers });
  if (res.status === 404) return null;
  if (!res.ok) throw new Error(`GitHub ${path}: ${res.status} ${await res.text()}`);
  return (await res.json()) as T;
}

// "<hash>  <name>" or "<hash> *<name>" per line, as sha256sum writes it.
export function parseSums(text: string): Map<string, string> {
  const sums = new Map<string, string>();
  for (const line of text.split(/\r?\n/)) {
    const m = /^([0-9a-f]{64})\s+\*?(.+)$/i.exec(line.trim());
    if (m) sums.set(m[2].trim(), m[1].toLowerCase());
  }
  return sums;
}

// Release tags of ytdlpd (v1.0.0 and on); older tags in the repo are the Electron app's.
export const isOurs = (tag: string) => Number(/^v?(\d+)/.exec(tag)?.[1] ?? 0) >= minMajor;
// Updater metadata (latest.yml, *.blockmap) is not something to download by hand.
const isInstaller = (name: string) => name !== 'SHA256SUMS.txt' && !/\.(yml|blockmap)$/i.test(name);

type GhAsset = { name: string; size: number; download_count: number; browser_download_url: string; url: string };
type GhRelease = { tag_name: string; name: string | null; published_at: string; html_url: string; draft: boolean; prerelease: boolean; assets: GhAsset[] };

async function main() {
  const meta = await api<{ stargazers_count: number; forks_count: number }>('');
  const releases = (await api<GhRelease[]>('/releases?per_page=100')) ?? [];
  const published = releases.filter((r) => !r.draft && isOurs(r.tag_name));
  const latestRel = published.find((r) => !r.prerelease) ?? published[0] ?? null;

  let latest: Stats['latest'] = null;
  if (latestRel) {
    const sumsAsset = latestRel.assets.find((a) => a.name === 'SHA256SUMS.txt');
    let sums = new Map<string, string>();
    if (sumsAsset) {
      // The API asset url works for private repos too; it needs the octet-stream accept header.
      const res = await fetch(sumsAsset.url, { headers: { ...headers, accept: 'application/octet-stream' } });
      if (res.ok) sums = parseSums(await res.text());
      else console.warn(`SHA256SUMS.txt: ${res.status}, hashes left empty`);
    }
    latest = {
      tag: latestRel.tag_name,
      name: latestRel.name || latestRel.tag_name,
      published_at: latestRel.published_at,
      url: latestRel.html_url,
      assets: latestRel.assets
        .filter((a) => isInstaller(a.name))
        .map((a) => ({ name: a.name, size: a.size, download_count: a.download_count, url: a.browser_download_url, sha256: sums.get(a.name) ?? null })),
    };
  }

  const stats: Stats = {
    repo,
    fetched_at: new Date().toISOString(),
    stars: meta?.stargazers_count ?? null,
    forks: meta?.forks_count ?? null,
    latest,
    total_downloads: published.flatMap((r) => r.assets).filter((a) => isInstaller(a.name)).reduce((n, a) => n + a.download_count, 0),
  };
  writeFileSync(out, JSON.stringify(stats, null, 2) + '\n');
  console.log(`stats: ${repo}, ${stats.stars ?? 'no'} stars, latest ${latest?.tag ?? 'none'}, ${stats.total_downloads} downloads`);
}

if (import.meta.main) await main();
