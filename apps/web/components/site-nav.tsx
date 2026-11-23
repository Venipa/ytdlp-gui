'use client';
import Link from 'next/link';
import { useEffect, useState } from 'react';
import { ThemeSwitch } from 'fumadocs-ui/layouts/shared/slots/theme-switch';
import { useOsAsset } from '@/components/download';
import { Mark } from '@/components/mark';
import { changelogUrl, repoUrl } from '@/lib/shared';
import { describe, num, osName, stats } from '@/lib/stats';

const assets = stats.latest?.assets ?? [];

const icon = (d: string) => (
  <svg width="20" height="20" viewBox="0 0 24 24" aria-hidden focusable="false" fill="none" stroke="currentColor" strokeWidth="1.6">
    <path d={d} />
  </svg>
);
const tabs = [
  { label: 'Home', href: '/', icon: icon('M4 11 12 4l8 7v9h-5v-6H9v6H4z') },
  { label: 'Docs', href: '/docs', icon: icon('M6 3h9l3 3v15H6zM9 10h6M9 14h6M9 18h4') },
  { label: 'Download', href: '#download', icon: icon('M12 4v11m-5-5 5 5 5-5M5 20h14') },
  { label: 'GitHub', href: repoUrl, icon: icon('M9 19c-4 1.5-4-2-6-2.5m12 5v-3.5c0-1 .1-1.4-.5-2 2.8-.3 5.5-1.4 5.5-6a4.6 4.6 0 0 0-1.3-3.2 4.2 4.2 0 0 0-.1-3.2s-1.1-.3-3.5 1.3a12 12 0 0 0-6.2 0C6.5 2.8 5.4 3.1 5.4 3.1a4.2 4.2 0 0 0-.1 3.2A4.6 4.6 0 0 0 4 9.5c0 4.6 2.7 5.7 5.5 6-.6.6-.6 1.2-.5 2V21') },
];

// Landing page nav: a floating bar that gains a fill once the page scrolls, plus a bottom tab bar on phones.
export function SiteNav() {
  const [scrolled, setScrolled] = useState(false);
  const asset = useOsAsset(assets);
  const os = asset && describe(asset.name).os;

  useEffect(() => {
    const on = () => setScrolled(window.scrollY > 8);
    on();
    window.addEventListener('scroll', on, { passive: true });
    return () => window.removeEventListener('scroll', on);
  }, []);

  return (
    <>
      <header className="sn" data-scrolled={scrolled || undefined}>
        <nav aria-label="Main" className="sn-bar">
          <Link href="/" className="sn-brand" aria-current="page" aria-label="ytdlpd home">
            <Mark />
            <span className="sn-name">ytdlpd</span>
          </Link>
          <div className="sn-more">
            <Link href="/docs" className="sn-link">
              Docs
            </Link>
            <a href={changelogUrl} className="sn-link">
              Changelog
            </a>
            <ThemeSwitch className="sn-theme" />
            <a href={repoUrl} className="sn-btn sn-quiet" aria-label={stats.stars ? `GitHub, ${num(stats.stars)} stars` : undefined}>
              GitHub {stats.stars ? <span className="sn-stars">&#9733; {num(stats.stars)}</span> : null}
            </a>
            <a
              href={asset?.url ?? '#download'}
              className="sn-btn"
              title={os ? `Download for ${osName[os]}` : 'Go to the download section'}
            >
              Download
            </a>
          </div>
        </nav>
      </header>
      <nav aria-label="Tabs" className="sn-tabs">
        {tabs.map((t) => (
          <Link key={t.label} href={t.href} aria-current={t.href === '/' ? 'page' : undefined}>
            {t.icon}
            {t.label}
          </Link>
        ))}
      </nav>
    </>
  );
}
