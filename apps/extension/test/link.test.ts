import { describe, expect, test } from "bun:test";
import { linkTarget } from "../src/link";

const PAGE = "https://site.example/list";
const box = (width: number, height: number) => ({ width, height });
const L = "https://site.example/watch?v=1";

describe("linkTarget", () => {
  test("rich: a big thumbnail, or a big link without one", () => {
    expect(linkTarget(L, PAGE, box(300, 60), box(320, 180))).toEqual({ kind: "rich", thumb: true });
    expect(linkTarget(L, PAGE, box(260, 140))).toEqual({ kind: "rich", thumb: false });
    expect(linkTarget(L, PAGE, box(400, 200), box(60, 60))).toEqual({ kind: "rich", thumb: false });
  });
  test("small: 48x48 up to the rich size, on the thumbnail when there is one", () => {
    expect(linkTarget(L, PAGE, box(300, 70), box(120, 68))).toEqual({ kind: "small", thumb: true });
    expect(linkTarget(L, PAGE, box(48, 48))).toEqual({ kind: "small", thumb: false });
    expect(linkTarget(L, PAGE, box(239, 134))).toEqual({ kind: "small", thumb: false });
  });
  test("none: text links and tiny thumbnails", () => {
    expect(linkTarget(L, PAGE, box(200, 20))).toBeNull();
    expect(linkTarget(L, PAGE, box(300, 40), box(47, 47))).toBeNull();
    expect(linkTarget(L, PAGE, box(47, 200))).toBeNull();
  });
  test("none: the same page, with or without a fragment", () => {
    expect(linkTarget(PAGE, PAGE, box(400, 300))).toBeNull();
    expect(linkTarget(PAGE + "#top", PAGE, box(400, 300))).toBeNull();
    expect(linkTarget(L + "#t=5", PAGE, box(400, 300))).not.toBeNull();
  });
  test("none: not http(s)", () => {
    for (const h of ["javascript:void(0)", "mailto:a@b.example", "data:text/html,x", "ftp://x.example/a", "", "not a url"]) {
      expect(linkTarget(h, PAGE, box(400, 300))).toBeNull();
    }
  });
  test("none: too long", () => {
    expect(linkTarget("https://a.example/" + "x".repeat(2100), PAGE, box(400, 300))).toBeNull();
  });
});
