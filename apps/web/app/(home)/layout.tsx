import { SiteNav } from '@/components/site-nav';

export default function Layout({ children }: LayoutProps<'/'>) {
  return (
    <>
      <SiteNav />
      {children}
    </>
  );
}
