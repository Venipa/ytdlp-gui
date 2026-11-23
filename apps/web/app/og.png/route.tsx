import { readFile } from 'node:fs/promises';
import { join } from 'node:path';
import { ImageResponse } from 'next/og';

export const dynamic = 'force-static';
// A route, not opengraph-image.tsx: that exports an extensionless file, which GitHub Pages serves as octet-stream.
export const size = { width: 1200, height: 630 };

// Landing page dark palette (global.css --l-*).
const ground = '#15130f';
const drawer = '#1e1b16';
const edge = '#2a2620';
const paper = '#ece5d8';
const pencil = '#9c9384';
const amber = '#e9a23b';

const font = (file: string) => readFile(join(process.cwd(), '../desktop/assets/fonts', file));

export async function GET() {
  const [sans, semi, mono] = await Promise.all([
    font('IBMPlexSans-Regular.ttf'),
    font('IBMPlexSans-SemiBold.ttf'),
    font('IBMPlexMono-Regular.ttf'),
  ]);
  return new ImageResponse(
    (
      <div
        style={{
          width: '100%',
          height: '100%',
          display: 'flex',
          flexDirection: 'column',
          justifyContent: 'space-between',
          padding: 80,
          background: ground,
          // faint ledger rules, like the page's tables
          backgroundImage: `linear-gradient(${edge} 1px, transparent 1px)`,
          backgroundSize: '100% 56px',
          color: paper,
          fontFamily: 'Plex Sans',
        }}
      >
        <div style={{ display: 'flex', alignItems: 'center', gap: 20 }}>
          <svg width="64" height="64" viewBox="0 0 22 22">
            <rect x="0.5" y="0.5" width="21" height="21" rx="2" fill={drawer} stroke={edge} />
            <path d="M11 5v9m-4-4 4 4 4-4M6.5 16.5h9" fill="none" stroke={amber} strokeWidth="1.6" />
          </svg>
          <div style={{ fontFamily: 'Plex Mono', fontSize: 34, color: pencil }}>ytdlpd</div>
        </div>
        <div style={{ display: 'flex', flexDirection: 'column', gap: 24 }}>
          <div style={{ fontSize: 68, fontWeight: 600, lineHeight: 1.08, letterSpacing: -1.5, maxWidth: 900 }}>
            A native desktop downloader for yt-dlp.
          </div>
          <div style={{ fontSize: 30, color: pencil, maxWidth: 880, lineHeight: 1.4 }}>
            Paste a link, pick a preset, get a correctly named file.
          </div>
          <div style={{ display: 'flex', gap: 14, alignItems: 'center', fontFamily: 'Plex Mono', fontSize: 22, color: pencil }}>
            <div style={{ width: 10, height: 10, borderRadius: 5, background: amber }} />
            <span>Windows · macOS · Linux</span>
          </div>
        </div>
      </div>
    ),
    {
      ...size,
      fonts: [
        { name: 'Plex Sans', data: sans, weight: 400 },
        { name: 'Plex Sans', data: semi, weight: 600 },
        { name: 'Plex Mono', data: mono, weight: 400 },
      ],
    },
  );
}
