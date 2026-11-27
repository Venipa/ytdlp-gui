import { expect, test } from "bun:test";
import { readdirSync, readFileSync } from "node:fs";
import { join } from "node:path";
import { ERROR_CODES } from "../src/protocol";

const root = join(import.meta.dir, "..");
const messages = JSON.parse(readFileSync(join(root, "_locales", "en", "messages.json"), "utf8"));

test("every error code has an err_ string", () => {
  const missing = ERROR_CODES.filter((c) => !messages["err_" + c]);
  expect(missing).toEqual([]);
});

test("every string key used in the source exists", () => {
  const used = new Set<string>();
  const src = join(root, "src");
  for (const f of readdirSync(src).filter((n) => /\.(ts|html)$/.test(n))) {
    const text = readFileSync(join(src, f), "utf8");
    for (const m of text.matchAll(/\b(?:t|getMessage)\(\s*"([a-z_]+)"\s*[,)]/g)) used.add(m[1]!);
    for (const m of text.matchAll(/LABEL|data-i18n(?:-label)?="([a-z_]+)"/g)) if (m[1]) used.add(m[1]);
    for (const m of text.matchAll(/key: "([a-z_]+)"/g)) used.add(m[1]!);
  }
  const manifest = readFileSync(join(root, "manifest.chrome.json"), "utf8");
  for (const m of manifest.matchAll(/__MSG_([a-z_]+)__/g)) used.add(m[1]!);
  const missing = [...used].filter((k) => !messages[k]);
  expect(missing).toEqual([]);
  expect(used.size).toBeGreaterThan(30);
});
