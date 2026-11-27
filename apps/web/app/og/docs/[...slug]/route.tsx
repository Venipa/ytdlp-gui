import { readFile } from 'node:fs/promises';
import { join } from 'node:path';
import { notFound } from 'next/navigation';
import { ogImage } from '@/lib/og';
import { basePath, siteUrl } from '@/lib/shared';
import { source } from '@/lib/source';

export const dynamic = 'force-static';
export const dynamicParams = false;

/** The link preview image of a docs page, at og/docs/<slug>/og.png (og/docs/og.png for the docs home). */
export async function GET(_req: Request, { params }: { params: Promise<{ slug: string[] }> }) {
  const { slug } = await params;
  const page = source.getPage(slug.slice(0, -1));
  if (!page) notFound();
  // the page's first screenshot, if it has one
  const mdx = await readFile(join(process.cwd(), 'content/docs', page.path), 'utf8');
  const shot = /assets\/shots\/([\w-]+)\.webp/.exec(mdx)?.[1];
  return ogImage({
    section: 'ytdlpd docs',
    title: page.data.title,
    description: page.data.description,
    url: `${siteUrl}${basePath}${page.url}`.replace(/^https?:\/\//, ''),
    shot,
  });
}

export function generateStaticParams() {
  return source.getPages().map((page) => ({ slug: [...page.slugs, 'og.png'] }));
}
