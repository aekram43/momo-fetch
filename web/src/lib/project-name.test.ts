import { describe, expect, it } from "vitest";

import { projectName } from "./project-name";

describe("projectName", () => {
  it("takes the last segment", () => {
    expect(projectName("/Users/me/workspace/momo-assistant")).toBe("momo-assistant");
  });

  it("ignores a trailing separator", () => {
    expect(projectName("/Users/me/workspace/momo-assistant/")).toBe("momo-assistant");
  });

  it("handles windows paths", () => {
    expect(projectName("C:\\Users\\me\\momo-assistant")).toBe("momo-assistant");
  });

  it("falls back to the path when there is no folder name", () => {
    // Rooted at "/" — a dot would be no better, and an empty label collapses
    // the line to the blank space this replaced.
    expect(projectName("/")).toBe("/");
  });

  it("is null until the path is known", () => {
    // Settings are fetched; the label must not flash a wrong value first.
    expect(projectName(undefined)).toBeNull();
    expect(projectName("")).toBeNull();
  });
});
