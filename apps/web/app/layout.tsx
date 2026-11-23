import localFont from 'next/font/local';
import type { Metadata } from 'next';
import { Provider } from '@/components/provider';
import './global.css';

// IBM Plex, self-hosted from the desktop app's own font files (OFL).
const sans = localFont({
  variable: '--font-plex-sans',
  src: [
    { path: '../../desktop/assets/fonts/IBMPlexSans-Regular.ttf', weight: '400' },
    { path: '../../desktop/assets/fonts/IBMPlexSans-Medium.ttf', weight: '500' },
    { path: '../../desktop/assets/fonts/IBMPlexSans-SemiBold.ttf', weight: '600' },
  ],
});
const mono = localFont({
  variable: '--font-plex-mono',
  preload: false,
  src: [
    { path: '../../desktop/assets/fonts/IBMPlexMono-Regular.ttf', weight: '400' },
    { path: '../../desktop/assets/fonts/IBMPlexMono-Medium.ttf', weight: '500' },
  ],
});

export const metadata: Metadata = {
  title: { default: 'ytdlpd: a native desktop downloader for yt-dlp', template: '%s | ytdlpd' },
  description:
    'Paste a link, pick a preset, get a correctly named file. A native desktop app for anything yt-dlp supports, on Windows, macOS and Linux.',
};

export default function Layout({ children }: LayoutProps<'/'>) {
  return (
    <html lang="en" className={`${sans.variable} ${mono.variable}`} suppressHydrationWarning>
      <body className="flex flex-col min-h-screen">
        <Provider>{children}</Provider>
      </body>
    </html>
  );
}
