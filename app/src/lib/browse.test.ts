import { describe, expect, it } from "vitest";
import { browseStarts, pickStart } from "./browse";

describe("browseStarts", () => {
  it("keeps stored starts up to the current one and always adds the current one", () => {
    expect(browseStarts(["2026-10-04", "2026-10-07", "2026-10-05"], "2026-10-06")).toEqual(["2026-10-04", "2026-10-05", "2026-10-06"]);
    expect(browseStarts(["2026-10-06"], "2026-10-06")).toEqual(["2026-10-06"]);
    expect(browseStarts([], "2026-10-06")).toEqual(["2026-10-06"]);
  });
});

describe("pickStart", () => {
  const stored = ["2026-10-03", "2026-10-05"];
  it("keeps a reachable kept start", () => {
    expect(pickStart(stored, "2026-10-06", "2026-10-03")).toBe("2026-10-03");
    expect(pickStart(stored, "2026-10-06", "2026-10-06")).toBe("2026-10-06");
  });
  it("otherwise picks the current start when it has a digest, else the newest earlier one, else the current one", () => {
    expect(pickStart([...stored, "2026-10-06"], "2026-10-06", null)).toBe("2026-10-06");
    expect(pickStart(stored, "2026-10-06", null)).toBe("2026-10-05");
    expect(pickStart(stored, "2026-10-06", "2026-10-04")).toBe("2026-10-05");
    expect(pickStart(stored, "2026-10-06", "2026-10-09")).toBe("2026-10-05");
    expect(pickStart([], "2026-10-06", null)).toBe("2026-10-06");
  });
});
