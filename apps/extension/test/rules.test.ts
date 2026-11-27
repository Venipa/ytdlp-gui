import { expect, test } from "bun:test";
import { fixUrl } from "../src/rules";

test("beeg thumbnail links lose the padding zero", () => {
  expect(fixUrl("https://beeg.com/-01234567890123")).toBe("https://beeg.com/-1234567890123");
  expect(fixUrl("https://www.beeg.com/-0123456789012345?t=1")).toBe("https://www.beeg.com/-123456789012345?t=1");
});

test("real beeg links and other sites stay as they are", () => {
  expect(fixUrl("https://beeg.com/-1234567890123")).toBe("https://beeg.com/-1234567890123");
  expect(fixUrl("https://beeg.com/-012")).toBe("https://beeg.com/-012"); // too short for an id
  expect(fixUrl("https://beeg.com/-01234567890123456")).toBe("https://beeg.com/-01234567890123456"); // too long
  expect(fixUrl("https://notbeeg.com/-01234567890123")).toBe("https://notbeeg.com/-01234567890123");
  expect(fixUrl("https://www.youtube.com/watch?v=1")).toBe("https://www.youtube.com/watch?v=1");
  expect(fixUrl("not a url")).toBe("not a url");
});
