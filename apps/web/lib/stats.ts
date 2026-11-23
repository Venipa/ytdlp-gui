import raw from '@/data/stats.json';
import type { Asset, Stats } from '@/scripts/fetch-stats';

export type { Asset, Stats };
export const stats = raw as Stats;

export type Os = 'windows' | 'macos' | 'linux';

const nf = new Intl.NumberFormat('en-US');
export const num = (n: number) => nf.format(n);

export function formatSize(bytes: number): string {
  const mb = bytes / 1_000_000;
  return mb >= 1 ? `${mb.toFixed(1)} MB` : `${Math.max(1, Math.round(bytes / 1000))} KB`;
}

export const formatDate = (iso: string) => iso.slice(0, 10);

// Asset names come from the package scripts: ytdlpd-<v>-windows-x64-setup.exe, -macos-<arch>.dmg, -linux-<arch>.AppImage.
export function describe(name: string): { os: Os | null; arch: string; label: string } {
  const os: Os | null = /windows|\.exe$/i.test(name)
    ? 'windows'
    : /macos|\.dmg$/i.test(name)
      ? 'macos'
      : /linux|\.appimage$/i.test(name)
        ? 'linux'
        : null;
  const arch = /arm64|aarch64/i.test(name) ? 'arm64' : 'x64';
  const label =
    os === 'windows'
      ? `Windows ${arch} installer`
      : os === 'macos'
        ? `macOS ${arch === 'arm64' ? 'Apple silicon' : 'Intel'}`
        : os === 'linux'
          ? `Linux ${arch} AppImage`
          : name;
  return { os, arch, label };
}

// Best asset for an OS: prefer arm64 on macOS (Apple silicon is the common case), x64 elsewhere.
export function pick(assets: Asset[], os: Os, arm = os === 'macos'): Asset | undefined {
  const forOs = assets.filter((a) => describe(a.name).os === os);
  return forOs.find((a) => (describe(a.name).arch === 'arm64') === arm) ?? forOs[0];
}

export const osName: Record<Os, string> = { windows: 'Windows', macos: 'macOS', linux: 'Linux' };
