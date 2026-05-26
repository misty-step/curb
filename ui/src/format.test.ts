import { describe, expect, it, vi } from "vitest";
import { formatDuration, formatTokens, relativeTime, stateLabel, statusTone } from "./format";

describe("format helpers", () => {
  it("formats token counts for dashboard scanning", () => {
    expect(formatTokens(9999)).toBe("9999");
    expect(formatTokens(55_323)).toBe("55k");
    expect(formatTokens(3_500_000)).toBe("3.5M");
  });

  it("formats durations compactly", () => {
    expect(formatDuration(42)).toBe("42s");
    expect(formatDuration(600)).toBe("10m");
    expect(formatDuration(7_500)).toBe("2h 5m");
  });

  it("formats relative time without exposing raw timestamps in tables", () => {
    vi.setSystemTime(new Date("2026-05-21T12:00:00Z"));
    expect(relativeTime("2026-05-21T11:54:00Z")).toBe("6m ago");
    vi.useRealTimers();
  });

  it("keeps usage severity separate from process state", () => {
    expect(stateLabel("watch-only", "stop")).toBe("watch-only / stop");
    expect(statusTone("ACTION")).toBe("action");
  });
});
