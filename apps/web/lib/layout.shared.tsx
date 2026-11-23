import type { BaseLayoutProps } from 'fumadocs-ui/layouts/shared';
import { Mark } from '@/components/mark';
import { appName, changelogUrl, repoUrl } from './shared';

export function baseOptions(): BaseLayoutProps {
  return {
    nav: {
      title: (
        <>
          <Mark />
          <span className="font-medium">{appName}</span>
        </>
      ),
    },
    githubUrl: repoUrl,
    links: [
      { text: 'Docs', url: '/docs', active: 'url' },
      { text: 'Download', url: '/#download' },
      { text: 'Changelog', url: changelogUrl, external: true },
    ],
  };
}
