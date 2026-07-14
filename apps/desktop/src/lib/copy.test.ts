import { describe, expect, it } from "vitest";
import { BANNED_PHRASES, PRODUCT_COPY } from "./copy";

describe("product framing has no medical-treatment language", () => {
  const allCopy = Object.values(PRODUCT_COPY).join(" ").toLowerCase();

  for (const banned of BANNED_PHRASES) {
    it(`does not contain "${banned}"`, () => {
      expect(allCopy).not.toContain(banned);
    });
  }

  it("states it is not a health intervention", () => {
    expect(allCopy).toContain("not a health intervention");
  });

  it("states stimulation does not reproduce third-party products", () => {
    expect(allCopy).toContain("does not reproduce any third-party product");
  });
});
