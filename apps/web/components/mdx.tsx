import defaultMdxComponents from 'fumadocs-ui/mdx';
import type { MDXComponents } from 'mdx/types';
import type { ReactNode } from 'react';
import { repoUrl } from '@/lib/shared';

// Links into the GitHub repository, so the repo name lives only in site.config.mjs.
function Repo({ path = '', children }: { path?: string; children: ReactNode }) {
  return <a href={repoUrl + path}>{children}</a>;
}

export function getMDXComponents(components?: MDXComponents) {
  return {
    ...defaultMdxComponents,
    Repo,
    ...components,
  } satisfies MDXComponents;
}

export const useMDXComponents = getMDXComponents;

declare global {
  type MDXProvidedComponents = ReturnType<typeof getMDXComponents>;
}
