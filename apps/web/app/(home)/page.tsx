import Image from 'next/image';
import Link from 'next/link';
import type { CSSProperties } from 'react';
import { DownloadButton } from '@/components/download';
import { describe, formatDate, formatSize, num, stats } from '@/lib/stats';
import { repoUrl } from '@/lib/shared';
import heroShot from '@/assets/shots/downloads-dark.webp';

const latest = stats.latest;
const version = latest?.tag.replace(/^v/, '') ?? '';
const plural = (n: number, one: string) => `${num(n)} ${one}${n === 1 ? '' : 's'}`;

// The three rows that are downloading in the capture: their rule's top edge and the share already drawn.
const moving = [
  { top: '24.69%', from: '10%', dur: '9s' },
  { top: '28.66%', from: '19%', dur: '7s' },
  { top: '32.63%', from: '14%', dur: '11s' },
];

const features = [
  ['Queue with pause and resume', 'Three at a time, two per site. After a restart, unfinished work waits, paused.'],
  ['Presets with real conversion', 'Video best down to 480p; audio as mp3, m4a or opus.'],
  ['Tools fetched for you', 'ffmpeg and a JavaScript runtime are downloaded when missing; yt-dlp updates without a new app.'],
  ['Files that play everywhere', "Each finished file is checked; one that won't play in QuickTime, Photos or Windows is one click from compatible."],
  ['Site rules', 'Per site: what a copied link does, which preset it gets and which fix runs after it.'],
  ['Cookies per site', "Import a file, paste one or copy a browser's; each download only gets its own site's cookies."],
];

const columns = ['ytdlpd', 'Old Electron app', 'yt-dlp CLI'];

const comparison: [string, string, string, string][] = [
  ['Runs in', 'A native window (Rust, GPUI)', 'Electron (Chromium)', 'A terminal'],
  ['Memory, idle', '124 MB in 1 process', '291 MB in 4 processes', 'Nothing running'],
  ['Talks to yt-dlp through', 'Its Python API, with typed progress hooks', 'Parsing its text output', 'It is yt-dlp'],
  [
    'Final file path',
    "Taken from yt-dlp's post hooks",
    'Guessed; a merge or convert that changes the extension shows as an error',
    'Printed when you ask for it',
  ],
  [
    'Queue',
    'Global and per-site limits; pause, resume, cancel and retry in place',
    'Queue waiters leak',
    'One command at a time, or your own script',
  ],
  ['Progress updates', 'At most four a second, latest wins', 'Re-renders on every tick', 'One terminal line'],
  ['Clipboard', 'Reads only; site rules decide whether a link is added, offered or ignored', 'Wipes the clipboard', 'Not involved'],
  ['yt-dlp options', 'The common ones as settings; no free-form arguments', 'Free-form, split on spaces', 'Every option'],
  ['Site support and speed', 'Same as yt-dlp', 'Same as yt-dlp', 'Same as yt-dlp'],
];

function StatsLine() {
  const parts: string[] = [];
  if (stats.stars) parts.push(plural(stats.stars, 'star'));
  if (latest) {
    parts.push(`${latest.tag} released ${formatDate(latest.published_at)}`);
    parts.push(plural(stats.total_downloads, 'download'));
  } else {
    parts.push('first release coming soon');
  }
  const line = parts.join(', ');
  return <p className="quiet tab">{line[0].toUpperCase() + line.slice(1)}</p>;
}

function BuildFromSource() {
  return (
    <Link className="btn" href="/docs/building-from-source">
      Build from source
    </Link>
  );
}

export default function HomePage() {
  return (
    <main className="lp flex-1">
      <section className="wrap hero">
        <div>
          <h1>Every download, kept in order.</h1>
          <p className="lede">
            Paste a link, pick a preset, get a correctly named file. A native desktop app for anything yt-dlp
            supports, on Windows, macOS and Linux.
          </p>
          {latest?.assets.length ? <DownloadButton assets={latest.assets} version={version} /> : <BuildFromSource />}
          <StatsLine />
        </div>
        <div className="shotbox">
          <Image
            src={heroShot}
            alt="The ytdlpd Downloads page: a link field, filter chips and a numbered list of downloads, three of them in progress with amber progress lines."
            priority
            sizes="(max-width: 960px) 100vw, 820px"
          />
          {moving.map((m) => (
            <i key={m.top} className="ov" aria-hidden style={{ top: m.top, '--from': m.from, '--dur': m.dur } as CSSProperties} />
          ))}
        </div>
      </section>

      <section className="strip" aria-label="Highlights">
        <ol className="wrap">
          {features.map(([title, text], i) => (
            <li key={title}>
              <span className="cn">{String(i + 1).padStart(4, '0')}</span>
              <span>{title}</span>
              <p>{text}</p>
            </li>
          ))}
        </ol>
      </section>

      <section className="wrap block" id="compare">
        <h2>How it compares</h2>
        <p className="sub">
          ytdlpd replaces ytdlp-desktop, an Electron app, and runs yt-dlp underneath. Sites and download speed are
          yt-dlp&apos;s in all three columns.
        </p>
        <div className="tablewrap">
          <table className="cmp">
            <thead>
              <tr>
                <th scope="col">
                  <span className="sr-only">Topic</span>
                </th>
                {columns.map((c) => (
                  <th scope="col" key={c}>
                    {c}
                  </th>
                ))}
              </tr>
            </thead>
            <tbody>
              {comparison.map(([topic, ...cells]) => (
                <tr key={topic}>
                  <th scope="row">{topic}</th>
                  {cells.map((c, i) => (
                    <td key={i} data-label={columns[i]}>
                      {c}
                    </td>
                  ))}
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      </section>

      <section className="wrap block" id="download">
        <h2>Download</h2>
        <p className="sub">
          Windows, macOS and Linux. yt-dlp comes with the app; ffmpeg and a JavaScript runtime are fetched on first
          run when your computer has none.
        </p>
        {latest?.assets.length ? (
          <>
            <DownloadButton assets={latest.assets} version={version} />
            <details>
              <summary>Show all files and checksums</summary>
              <div className="tablewrap">
                <table className="assets">
                  <thead>
                    <tr>
                      <th scope="col">File</th>
                      <th scope="col" className="hide-sm">
                        Platform
                      </th>
                      <th scope="col" className="num">
                        Size
                      </th>
                      <th scope="col">SHA-256</th>
                    </tr>
                  </thead>
                  <tbody>
                    {latest.assets.map((a) => (
                      <tr key={a.name}>
                        <td className="name">
                          <a className="link" href={a.url}>
                            {a.name}
                          </a>
                        </td>
                        <td className="hide-sm">{describe(a.name).label}</td>
                        <td className="num">{formatSize(a.size)}</td>
                        <td className="hash">{a.sha256 ?? 'Not published'}</td>
                      </tr>
                    ))}
                  </tbody>
                </table>
              </div>
              <p className="verify">
                Check a file with <code>sha256sum &lt;file&gt;</code> on Linux, <code>shasum -a 256 &lt;file&gt;</code> on
                macOS or <code>Get-FileHash &lt;file&gt;</code> in PowerShell. Older versions are on{' '}
                <a className="link" href={`${repoUrl}/releases`}>
                  GitHub releases
                </a>
                .
              </p>
            </details>
          </>
        ) : (
          <>
            <p>First release coming soon.</p>
            <p className="verify">
              Until then you can{' '}
              <Link className="link" href="/docs/building-from-source">
                build ytdlpd from source
              </Link>{' '}
              on Windows, macOS or Linux.
            </p>
          </>
        )}
      </section>

      <footer>
        <div className="wrap">
          <span>ytdlpd is free software under GPL-3.0-or-later.</span>
          <Link className="link" href="/docs">
            Docs
          </Link>
          <Link className="link" href="/docs/privacy-and-licences">
            Privacy and licences
          </Link>
          <a className="link" href={repoUrl}>
            Source on GitHub
          </a>
        </div>
      </footer>
    </main>
  );
}
