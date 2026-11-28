import { expect, test } from "bun:test";
import { readFileSync } from "node:fs";
import { join } from "node:path";
import { linkTarget } from "../src/link";

// content.ts and bar.ts need a DOM and the chrome API, which bun test has not: these checks read the source.
const src = (f: string) => readFileSync(join(import.meta.dir, "..", "src", f), "utf8");

test("the worker locks storage to trusted contexts", () => {
  expect(src("background.ts")).toMatch(/storage\.local\.setAccessLevel\?\.\(\{ accessLevel: "TRUSTED_CONTEXTS" \}\)/);
});

test("no content-script path touches storage or the client", () => {
  for (const f of ["content.ts", "bar.ts", "hidden.ts"]) {
    const text = src(f);
    expect(text).not.toMatch(/chrome\.storage/);
    expect(text).not.toMatch(/from "\.\/client"/);
  }
  expect(src("content.ts")).not.toMatch(/"tokens"|\.tokens/);
});

test("fake pointer and focus events from the page are ignored, the host is always mounted", () => {
  const c = src("content.ts");
  expect(c).toMatch(/function onPointer\(e: PointerEvent\) \{\s*if \(!e\.isTrusted/);
  expect(c).toMatch(/"focusin", \(e\) => \{\s*if \(!e\.isTrusted\) return;/);
  expect(c).toMatch(/^document\.documentElement\.appendChild\(bar\.host\);/m);
});

test("hidden checks and the menu host come from the worker, not this frame's location", () => {
  expect(src("content.ts")).not.toMatch(/isHidden|hostOf/);
  expect(src("bar.ts")).not.toMatch(/location\.hostname/);
});

test("a hover-preview <video> over a link card sends that link, not the page url (YouTube home)", () => {
  const c = src("content.ts");
  // the video is a preview when an anchor to another page sits under it (or around it) in the hit stack
  expect(c).toMatch(/anchors\.slice\(vi\)\.find\(\(n\) => n && linkTarget\(n\.href, location\.href/);
  expect(c).toMatch(/const v = under \|\| vi < 0 \? undefined/);
  expect(c).toMatch(/const a = v \? undefined : under \?\? anchors\.find\(Boolean\)/);
});

test("the YouTube home card link (resolved /watch?v=abc) is a valid target on https://www.youtube.com/", () => {
  const href = new URL("/watch?v=abc", "https://www.youtube.com/").href;
  expect(href).toBe("https://www.youtube.com/watch?v=abc");
  expect(linkTarget(href, "https://www.youtube.com/", { width: 360, height: 202 })).toEqual({ kind: "rich", thumb: false });
  expect(linkTarget(href, href, { width: 360, height: 202 })).toBeNull(); // on the watch page itself the video is the page's player
});
