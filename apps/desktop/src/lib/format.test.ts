import { describe, expect, it } from "vitest";
import { INTENSITY_LABELS, formatDuration, intensityLevel } from "./format";

describe("formatDuration", () => {
  it("formats seconds as M:SS", () => {
    expect(formatDuration(0)).toBe("0:00");
    expect(formatDuration(5)).toBe("0:05");
    expect(formatDuration(65)).toBe("1:05");
    expect(formatDuration(599)).toBe("9:59");
  });

  it("formats hours as H:MM:SS", () => {
    expect(formatDuration(3600)).toBe("1:00:00");
    expect(formatDuration(3661)).toBe("1:01:01");
    expect(formatDuration(7325)).toBe("2:02:05");
  });

  it("clamps negative input to zero", () => {
    expect(formatDuration(-10)).toBe("0:00");
  });
});

describe("intensity indicators", () => {
  it("provides non-colour level numbers 0..3", () => {
    expect(intensityLevel("off")).toBe(0);
    expect(intensityLevel("low")).toBe(1);
    expect(intensityLevel("medium")).toBe(2);
    expect(intensityLevel("high")).toBe(3);
  });

  it("labels High without claiming medical treatment", () => {
    expect(INTENSITY_LABELS.high).toContain("ADHD");
    expect(INTENSITY_LABELS.high).not.toContain("treat");
    expect(INTENSITY_LABELS.high).not.toContain("cure");
    expect(INTENSITY_LABELS.high).not.toContain("therapy");
  });
});
