import { describe, expect, it } from "vitest";

import {
  ORIGIN_MARKS,
  originKind,
  originLabel,
  originName,
} from "./session-origin";

describe("originKind", () => {
  it("reads the three stamped kinds", () => {
    expect(originKind("agent:reviewer")).toBe("agent");
    expect(originKind("worker:validator")).toBe("worker");
    expect(originKind("routine:Nightly digest")).toBe("routine");
  });

  it("treats an unstamped session as a chat", () => {
    expect(originKind(null)).toBe("chat");
  });

  it("falls back to chat rather than guessing at a kind it doesn't know", () => {
    // A gateway that grows a fourth kind must render as unmarked here, never
    // as the nearest one it happens to sort next to.
    expect(originKind("swarm:alpha")).toBe("chat");
    expect(originKind("routine")).toBe("chat");
    expect(originKind("")).toBe("chat");
  });
});

describe("originName", () => {
  it("keeps colons that belong to the name", () => {
    expect(originName("routine:03:00 sweep")).toBe("03:00 sweep");
  });

  it("is empty when there is no name to take", () => {
    expect(originName("worker")).toBe("");
  });
});

describe("originLabel", () => {
  it("says what started the session, in words", () => {
    expect(originLabel("agent:reviewer")).toBe("Started as agent reviewer");
    expect(originLabel("worker:validator")).toBe("Team worker validator");
    expect(originLabel("routine:Nightly digest")).toBe(
      "Routine Nightly digest",
    );
  });

  it("says nothing about an ordinary chat", () => {
    expect(originLabel(null)).toBeNull();
    expect(originLabel("swarm:alpha")).toBeNull();
  });
});

describe("ORIGIN_MARKS", () => {
  it("leaves chats unmarked and gives every other kind its own glyph", () => {
    expect(ORIGIN_MARKS.chat).toBeNull();
    const glyphs = (["agent", "worker", "routine"] as const).map(
      (k) => ORIGIN_MARKS[k]?.glyph,
    );
    expect(new Set(glyphs).size).toBe(glyphs.length);
    expect(glyphs.every(Boolean)).toBe(true);
  });
});
