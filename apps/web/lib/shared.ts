export const appName = 'ytdlpd';
export const docsRoute = '/docs';
export const basePath = process.env.NEXT_PUBLIC_BASE_PATH ?? '';

// Set from site.config.mjs by next.config.mjs.
export const repo = process.env.NEXT_PUBLIC_SITE_REPO!;
export const repoUrl = `https://github.com/${repo}`;
export const changelogUrl = `${repoUrl}/releases`;
