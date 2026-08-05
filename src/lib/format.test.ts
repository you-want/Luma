import { describe, expect, it } from "vitest";
import { basename, formatBytes, formatDate, formatNumber } from "./format";

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
