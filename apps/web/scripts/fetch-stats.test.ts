import { expect, test } from 'bun:test';
import { isOurs, parseSums } from './fetch-stats';

test('parseSums reads sha256sum output', () => {
  const h = 'a'.repeat(64);
  const sums = parseSums(`${h}  ytdlpd-0.1.0-windows-x64-setup.exe\r\n${'B'.repeat(64)} *ytdlpd-0.1.0-macos-arm64.dmg\nnot a line\n`);
  expect(sums.get('ytdlpd-0.1.0-windows-x64-setup.exe')).toBe(h);
  expect(sums.get('ytdlpd-0.1.0-macos-arm64.dmg')).toBe('b'.repeat(64));
  expect(sums.size).toBe(2);
});

test('isOurs skips the Electron app tags', () => {
  expect(isOurs('v0.15.3')).toBe(false);
  expect(isOurs('v2.0.0')).toBe(true);
  expect(isOurs('2.1.0-beta.1')).toBe(true);
});
