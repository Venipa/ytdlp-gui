// The one place that names the GitHub repository. Override with SITE_REPO=owner/name (and BASE_PATH).
export const repo = process.env.SITE_REPO || 'Venipa/ytdlp-gui';
// GitHub Pages project site lives under /<name>; BASE_PATH="" serves from the domain root.
export const basePath = process.env.BASE_PATH ?? `/${repo.split('/')[1]}`;
// The repo also holds the old Electron app's releases (v0.x); only tags from this major on are ytdlpd.
export const minMajor = 1;
