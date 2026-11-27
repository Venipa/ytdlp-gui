import { ogImage } from '@/lib/og';
import { basePath, siteUrl } from '@/lib/shared';

export const dynamic = 'force-static';
// A route, not opengraph-image.tsx: that exports an extensionless file, which GitHub Pages serves as octet-stream.

export function GET() {
  return ogImage({
    section: 'ytdlpd',
    title: 'Every download, kept in order.',
    description: 'A native desktop downloader for yt-dlp. Windows, macOS and Linux.',
    url: `${siteUrl}${basePath}`.replace(/^https?:\/\//, ''),
    shot: 'downloads-dark',
  });
}
