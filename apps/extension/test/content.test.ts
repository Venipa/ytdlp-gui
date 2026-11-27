import { expect, test } from "bun:test";
import { readFileSync } from "node:fs";
import { join } from "node:path";

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
