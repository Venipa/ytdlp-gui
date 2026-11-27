import { readFile } from 'node:fs/promises';
import { join } from 'node:path';
import { ImageResponse } from 'next/og';

/** Open Graph image size. */
export const ogSize = { width: 1200, height: 630 };

// Landing page dark palette (global.css --l-*).
const ground = '#15130f';
const drawer = '#1e1b16';
const edge = '#2a2620';
const paper = '#ece5d8';
const pencil = '#9c9384';
const amber = '#e9a23b';

const font = (file: string) => readFile(join(process.cwd(), '../desktop/assets/fonts', file));

/** What one link preview card shows. */
export type OgCard = {
  /** Mono label after the mark, e.g. "ytdlpd docs". */
  section: string;
  title: string;
  description?: string;
  /** Address shown at the bottom, without the scheme. */
  url: string;
  /** Screenshot name (assets/shots/<shot>.webp); without its card crop the text spans the card. */
  shot?: string;
};

/** Renders a link preview: text on ledger rules, the page's screenshot running off the right edge. */
export async function ogImage({ section, title, description, url, shot }: OgCard) {
  const [sans, semi, mono, img] = await Promise.all([
    font('IBMPlexSans-Regular.ttf'),
    font('IBMPlexSans-SemiBold.ttf'),
    font('IBMPlexMono-Regular.ttf'),
    // satori cannot decode webp: cards use 620x540 jpg crops, made with
    // magick assets/shots/<shot>.webp -resize 620x540^ -gravity northwest -extent 620x540 -quality 85 assets/og/<shot>.jpg
    // shortcut: a screenshot without a crop gives a text-only card; generate crops at build if that gets missed often.
    shot ? readFile(join(process.cwd(), 'assets/og', `${shot}.jpg`)).catch(() => undefined) : undefined,
  ]);
  const textWidth = img ? 520 : 1040;
  return new ImageResponse(
    (
      <div
        style={{
          width: '100%',
          height: '100%',
          display: 'flex',
          position: 'relative',
          background: ground,
          // faint ledger rules, like the page's tables
          backgroundImage: `linear-gradient(${edge} 1px, transparent 1px)`,
          backgroundSize: '100% 56px',
          color: paper,
          fontFamily: 'Plex Sans',
        }}
      >
        <div
          style={{
            display: 'flex',
            flexDirection: 'column',
            justifyContent: 'space-between',
            padding: '72px 0 72px 80px',
            width: textWidth + 80,
          }}
        >
          <div style={{ display: 'flex', alignItems: 'center', gap: 18 }}>
            <svg width="56" height="56" viewBox="0 0 22 22">
              <rect x="0.5" y="0.5" width="21" height="21" rx="2" fill={drawer} stroke={edge} />
              <path d="M11 5v9m-4-4 4 4 4-4M6.5 16.5h9" fill="none" stroke={amber} strokeWidth="1.6" />
            </svg>
            <div style={{ fontFamily: 'Plex Mono', fontSize: 28, color: pencil }}>{section}</div>
          </div>
          <div style={{ display: 'flex', flexDirection: 'column', gap: 20 }}>
            <div style={{ fontSize: img ? 64 : 76, fontWeight: 600, lineHeight: 1.05, letterSpacing: -1.5 }}>{title}</div>
            {description && (
              <div style={{ fontSize: img ? 26 : 30, color: pencil, lineHeight: 1.38, lineClamp: 4 }}>{description}</div>
            )}
          </div>
          <div style={{ display: 'flex', gap: 12, alignItems: 'center', fontFamily: 'Plex Mono', fontSize: 20, color: pencil }}>
            <div style={{ width: 10, height: 10, borderRadius: 5, background: amber }} />
            <span>{url}</span>
          </div>
        </div>
        {img && (
          <div
            style={{
              position: 'absolute',
              left: 660,
              top: 90,
              width: 620,
              height: 540,
              display: 'flex',
              overflow: 'hidden',
              border: `1px solid ${edge}`,
              boxShadow: '0 20px 60px rgba(0,0,0,0.65)',
            }}
          >
            {/* biome-ignore lint/performance/noImgElement: rendered by satori, not the browser */}
            <img
              src={`data:image/jpeg;base64,${img.toString('base64')}`}
              width={620}
              height={540}
            />
          </div>
        )}
      </div>
    ),
    {
      ...ogSize,
      fonts: [
        { name: 'Plex Sans', data: sans, weight: 400 },
        { name: 'Plex Sans', data: semi, weight: 600 },
        { name: 'Plex Mono', data: mono, weight: 400 },
      ],
    },
  );
}
