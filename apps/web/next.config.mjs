import { createMDX } from 'fumadocs-mdx/next';
import { basePath, repo, siteUrl } from './site.config.mjs';

const withMDX = createMDX();


/** @type {import('next').NextConfig} */
const config = {
  output: 'export',
  basePath,
  reactStrictMode: true,
  images: { unoptimized: true },
  env: { NEXT_PUBLIC_BASE_PATH: basePath, NEXT_PUBLIC_SITE_REPO: repo, NEXT_PUBLIC_SITE_URL: siteUrl },
};

export default withMDX(config);
