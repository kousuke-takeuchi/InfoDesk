import { describe, expect, it } from "vitest";
import { obj, stripMcpPrefix } from "./unwrap";

describe("unwrap helpers", () => {
  it("strips the infodesk MCP prefix only", () => {
    expect(stripMcpPrefix("mcp__infodesk__fetch_page")).toBe("fetch_page");
    expect(stripMcpPrefix("fetch_page")).toBe("fetch_page");
    expect(stripMcpPrefix("mcp__other__x")).toBe("mcp__other__x");
  });

  it("turns non-objects into an empty object", () => {
    expect(obj(null)).toEqual({});
    expect(obj("x")).toEqual({});
    expect(obj({ a: 1 })).toEqual({ a: 1 });
  });
});
