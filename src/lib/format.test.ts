import { beforeAll, describe, expect, it } from "vitest";
import {
  basename,
  formatBytes,
  formatDate,
  formatNumber,
  setFormatLocale,
} from "./format";

describe("formatBytes", () => {
  it("formats boundaries", () => {
    expect(formatBytes(0)).toBe("0 B");
    expect(formatBytes(1024)).toBe("1.00 KB");
    expect(formatBytes(10 * 1024 * 1024)).toBe("10.0 MB");
    expect(formatBytes(Number.NaN)).toBe("0 B");
    expect(formatBytes(-1)).toBe("0 B");
    expect(formatBytes(1024 ** 7)).toBe("1048576 PB");
  });
});

describe("localized values", () => {
  beforeAll(() => {
    // Use en-US in tests: assertions are locale-agnostic (digits only),
    // and en-US ICU data is always available instantly in CI environments.
    // zh-CN ICU loading takes 10+ seconds on Windows runners and hits the
    // default 5 s Vitest timeout.
    setFormatLocale("en-US");
  });

  it("formats counts and missing timestamps", () => {
    expect(formatNumber(12345).replace(/\D/g, "")).toBe("12345");
    expect(formatDate()).toBe("--");
    expect(formatDate(0)).toBe("--");
  });
});

describe("basename", () => {
  it("handles trailing separators", () => {
    expect(basename("/Users/demo/Documents/")).toBe("Documents");
  });
});
