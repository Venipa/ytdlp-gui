import type { BaseLayoutProps } from 'fumadocs-ui/layouts/shared';
import { appName, repoUrl } from './shared';

export function baseOptions(): BaseLayoutProps {
  return {
    nav: { title: <span className="font-medium">{appName}</span> },
    githubUrl: repoUrl,
    links: [
      { text: 'Docs', url: '/docs', active: 'url' },
      { text: 'Download', url: '/#download' },
    ],
  };
}
