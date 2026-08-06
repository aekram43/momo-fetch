import { describe, expect, it } from "vitest";

import { shortlistModels, SHORTLIST_SIZE } from "./recent-models";

const catalogue = ["a", "b", "c", "d", "e", "f", "g", "h"];

describe("shortlistModels", () => {
  it("shows the current model plus four others", () => {
    const list = shortlistModels(catalogue, "g", []);
    expect(list).toHaveLength(SHORTLIST_SIZE);
    expect(list[0]).toBe("g");
  });

  it("puts the current model first and recents next", () => {
    expect(shortlistModels(catalogue, "h", ["c", "a"])).toEqual([
      "h",
      "c",
      "a",
      "b",
      "d",
    ]);
  });

  it("never repeats a model that is both current and recent", () => {
    const list = shortlistModels(catalogue, "c", ["c", "e"]);
    expect(list).toEqual(["c", "e", "a", "b", "d"]);
    expect(new Set(list).size).toBe(list.length);
  });

  it("ignores recents the provider no longer lists", () => {
    // A model retired since it was last used must not be offered — switching to
    // it would fail at the provider.
    expect(shortlistModels(catalogue, "a", ["retired-model", "f"])).toEqual([
      "a",
      "f",
      "b",
      "c",
      "d",
    ]);
  });

  it("keeps the current model even when the catalogue omits it", () => {
    // The list must always be able to show what is actually running.
    expect(shortlistModels(["x", "y"], "not-listed", [])).toEqual([
      "not-listed",
      "x",
      "y",
    ]);
  });

  it("returns everything when the catalogue is shorter than the limit", () => {
    expect(shortlistModels(["x", "y"], null, [])).toEqual(["x", "y"]);
  });

  it("handles no current model", () => {
    expect(shortlistModels(catalogue, null, ["d"])).toEqual([
      "d",
      "a",
      "b",
      "c",
      "e",
    ]);
  });
});
