import localFont from 'next/font/local';
import type { Metadata, Viewport } from 'next';
import { Provider } from '@/components/provider';
import { appName, basePath, siteUrl } from '@/lib/shared';
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

const title = 'ytdlpd: a native desktop downloader for yt-dlp';
const description =
  'Paste a link, pick a preset, get a correctly named file. A native desktop app for anything yt-dlp supports, on Windows, macOS and Linux.';

// relative (no leading slash) so metadataBase keeps the base path
const og = { url: 'og.png', width: 1200, height: 630, alt: title };

export const metadata: Metadata = {
  // basePath included: relative URLs (og:image, icons) resolve under the Pages project path.
  metadataBase: new URL(`${siteUrl}${basePath}/`),
  title: { default: title, template: '%s | ytdlpd' },
  description,
  applicationName: appName,
  keywords: ['yt-dlp', 'video downloader', 'audio downloader', 'desktop app', 'Windows', 'macOS', 'Linux'],
  openGraph: { type: 'website', siteName: appName, title, description, locale: 'en_US', images: [og] },
  twitter: { card: 'summary_large_image', title, description, images: [og] },
  formatDetection: { telephone: false, email: false, address: false },
};

// cover lets env(safe-area-inset-bottom) reach the phone tab bar.
export const viewport: Viewport = {
  viewportFit: 'cover',
  colorScheme: 'light dark',
  themeColor: [
    { media: '(prefers-color-scheme: light)', color: '#f2ede3' },
    { media: '(prefers-color-scheme: dark)', color: '#15130f' },
  ],
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
