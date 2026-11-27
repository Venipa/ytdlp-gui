import { describe, expect, test } from "bun:test";
import { hide, hiddenBy, isHidden, pageKey, show } from "../src/hidden";

const u = "https://www.youtube.com/watch?v=abc#t=10";

describe("hidden sites", () => {
  test("page key ignores the hash", () => {
    expect(pageKey(u)).toBe("https://www.youtube.com/watch?v=abc");
  });

  test("hide on this page hides that page only", () => {
    const h = hide(undefined, "page", u);
    expect(isHidden(h, "https://www.youtube.com/watch?v=abcx")).toBe(false);
    expect(isHidden(h, "https://www.youtube.com/watch?v=abc#other")).toBe(true);
    expect(isHidden(h, "https://www.youtube.com/watch?v=def")).toBe(false);
    expect(hiddenBy(h, u)).toBe("page");
  });

  test("hide on a domain covers subdomains but not lookalikes", () => {
    const h = hide(undefined, "domain", "https://youtube.com/");
    expect(isHidden(h, "https://music.youtube.com/x")).toBe(true);
    expect(isHidden(h, "https://notyoutube.com/x")).toBe(false);
    expect(hiddenBy(h, "https://youtube.com/")).toBe("domain");
  });

  test("hiding twice does not duplicate", () => {
    const h = hide(hide(undefined, "domain", u), "domain", u);
    expect(h.domains).toEqual(["www.youtube.com"]);
  });

  test("show again lifts page and covering domain entries, keeps others", () => {
    let h = hide(undefined, "page", u);
    h = hide(h, "domain", "https://www.youtube.com/");
    h = hide(h, "domain", "https://vimeo.com/");
    const s = show(h, u);
    expect(isHidden(s, u)).toBe(false);
    expect(s.domains).toEqual(["vimeo.com"]);
  });

  test("garbage urls are not hidden", () => {
    expect(isHidden(hide(undefined, "domain", u), "not a url")).toBe(false);
  });
});
