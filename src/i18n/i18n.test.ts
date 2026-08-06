import { describe, expect, it } from "vitest";
import { enUS } from "./locales/en-US";
import { zhCN } from "./locales/zh-CN";

// Recursively collect the dotted key paths of every leaf string in a resource
// object. Two languages are complete iff they produce the identical key set.
function leafKeys(value: unknown, prefix = ""): string[] {
  if (value && typeof value === "object") {
    return Object.entries(value).flatMap(([key, child]) =>
      leafKeys(child, prefix ? `${prefix}.${key}` : key),
    );
  }
  return [prefix];
}

// Every `{{name}}` placeholder used in a resource string, so a translation can
// be checked for accidentally dropping or renaming an interpolation variable.
function placeholders(value: string): string[] {
  return [...value.matchAll(/\{\{(\w+)\}\}/g)].map((match) => match[1]).sort();
}

function flatten(value: unknown, prefix = ""): Map<string, string> {
  const out = new Map<string, string>();
  if (value && typeof value === "object") {
    for (const [key, child] of Object.entries(value)) {
      for (const [k, v] of flatten(child, prefix ? `${prefix}.${key}` : key)) {
        out.set(k, v);
      }
    }
  } else {
    out.set(prefix, String(value));
  }
  return out;
}

describe("i18n resource completeness", () => {
  it("zh-CN and en-US expose the identical key set", () => {
    const zh = leafKeys(zhCN).sort();
    const en = leafKeys(enUS).sort();
    expect(en).toEqual(zh);
  });

  it("no resource value is empty", () => {
    for (const [key, value] of flatten(zhCN)) {
      expect(value.length, `zh-CN ${key} is empty`).toBeGreaterThan(0);
    }
    for (const [key, value] of flatten(enUS)) {
      expect(value.length, `en-US ${key} is empty`).toBeGreaterThan(0);
    }
  });

  it("matching keys share the same interpolation placeholders", () => {
    const zh = flatten(zhCN);
    const en = flatten(enUS);
    for (const [key, zhValue] of zh) {
      const enValue = en.get(key) ?? "";
      expect(placeholders(enValue), `placeholders differ for ${key}`).toEqual(
        placeholders(zhValue),
      );
    }
  });
});
