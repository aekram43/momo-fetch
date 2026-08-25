import { describe, expect, it } from "vitest";

import { formatDistance, formatRelative } from "./relative-time";

const NOW = Date.parse("2026-08-25T12:00:00Z");
const iso = (offsetMs: number) => new Date(NOW + offsetMs).toISOString();

describe("formatRelative", () => {
  it("says which side of now it is on", () => {
    expect(formatRelative(iso(4 * 3600_000), NOW)).toBe("in 4h");
    expect(formatRelative(iso(-3 * 60_000), NOW)).toBe("3m ago");
  });

  it("collapses the seconds either side of now", () => {
    expect(formatRelative(iso(10_000), NOW)).toBe("now");
    expect(formatRelative(iso(-10_000), NOW)).toBe("now");
  });

  it("returns null for nothing to say, rather than a dash of its own", () => {
    expect(formatRelative(null, NOW)).toBeNull();
    expect(formatRelative("not a date", NOW)).toBeNull();
  });

  it("steps up a unit rather than getting more precise", () => {
    expect(formatDistance(90 * 60_000)).toBe("2h");
    expect(formatDistance(50 * 3600_000)).toBe("2d");
  });
});
