import { describe, expect, it, vi } from "vitest";
import { numberValue, relativeTime, tokens } from "./format";

describe("format helpers", () => {
  it("formats token counts on a compact scale", () => {
    expect(tokens(0)).toBe("0");
    expect(tokens(920)).toBe("920");
    expect(tokens(55_323)).toBe("55k");
    expect(tokens(3_500_000)).toBe("3.5M");
    expect(tokens(3_000_000)).toBe("3M");
  });

  it("formats relative time without exposing raw timestamps", () => {
    vi.setSystemTime(new Date("2026-05-29T12:00:00Z"));
    expect(relativeTime("2026-05-29T11:54:00Z")).toBe("6m ago");
    expect(relativeTime("2026-05-29T11:59:50Z")).toBe("just now");
    expect(relativeTime(undefined)).toBe("—");
    vi.useRealTimers();
  });

  it("coerces form values to finite numbers", () => {
    expect(numberValue("2000000")).toBe(2_000_000);
    expect(numberValue("")).toBe(0);
    expect(numberValue(42)).toBe(42);
  });
});
