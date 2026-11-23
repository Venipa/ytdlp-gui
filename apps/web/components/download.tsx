'use client';
import { useEffect, useState } from 'react';
import { type Asset, type Os, describe, formatSize, osName, pick } from '@/lib/stats';

type UaData = { getHighEntropyValues?: (hints: string[]) => Promise<{ architecture?: string }> };

// Picks the installer for the visitor's OS after mount; the static HTML renders the Windows one.
export function DownloadButton({ assets, version }: { assets: Asset[]; version: string }) {
  const [os, setOs] = useState<Os>('windows');
  const [arm, setArm] = useState<boolean | undefined>(undefined);

  useEffect(() => {
    const ua = navigator.userAgent;
    setOs(/Mac|iPhone|iPad/.test(ua) ? 'macos' : /Linux|X11|CrOS/.test(ua) && !/Android/.test(ua) ? 'linux' : 'windows');
    if (/aarch64|arm64/i.test(ua)) setArm(true);
    const uad = (navigator as Navigator & { userAgentData?: UaData }).userAgentData;
    uad?.getHighEntropyValues?.(['architecture']).then((v) => v.architecture && setArm(v.architecture === 'arm'), () => {});
  }, []);

  const asset = pick(assets, os, arm) ?? pick(assets, 'windows') ?? assets[0];
  if (!asset) return null;
  const d = describe(asset.name);

  return (
    <div>
      <a className="btn" href={asset.url}>
        Download for {d.os ? osName[d.os] : 'your computer'}
      </a>
      <span className="meta tab">
        Version {version}, {d.label}, {formatSize(asset.size)}
      </span>
    </div>
  );
}
