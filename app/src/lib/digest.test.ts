import { describe, expect, it } from "vitest";
import { digestDisplay, parseDigestBody } from "./digest";
import type { AppStatus, Digest, DigestView } from "../types";

const status: AppStatus = { collecting: false, summarizing: false, digesting: false, last_collect_at: null, next_collect_at: null, last_error: null };
const digest = (over: Partial<Digest>): Digest => ({
  id: 1, period: "day", period_start: "2026-10-06", scope: { category: "github" }, body: "", item_ids: [],
  provider: "ollama:gemma4:26b", status: "done", attempts: 0, created_at: "2026-10-06T00:00:00Z", last_error: null, ...over,
});
const view = (d: Digest | null, item_count = 0): DigestView => ({ scope: { category: "github" }, label: "GitHub", digest: d, item_count });

describe("parseDigestBody", () => {
  it("extracts bullets and keeps the rest as paragraphs", () => {
    const body = "- 一つ目。\n* 二つ目。\n・三つ目\n• 四つ目\n1. 五つ目\n\n全体として伸びている。";
    expect(parseDigestBody(body)).toEqual({
      bullets: ["一つ目。", "二つ目。", "三つ目", "四つ目", "五つ目"],
      paragraphs: ["全体として伸びている。"],
    });
    expect(parseDigestBody("")).toEqual({ bullets: [], paragraphs: [] });
  });
  it("skips empty bullets and marker-only lines", () => {
    expect(parseDigestBody("- \n*\n・\n•\n- 本文")).toEqual({ bullets: ["本文"], paragraphs: [] });
  });

  it("drops horizontal rules", () => {
    expect(parseDigestBody("---\n- 一つ\n***\n* * *\n段落")).toEqual({ bullets: ["一つ"], paragraphs: ["段落"] });
  });

  it("strips bold markers from bullets and paragraphs", () => {
    expect(parseDigestBody("- **重要**な点\n- __強調__です\n**全体**の傾向")).toEqual({
      bullets: ["重要な点", "強調です"],
      paragraphs: ["全体の傾向"],
    });
  });

  it("turns heading lines into paragraphs without hashes", () => {
    expect(parseDigestBody("# 見出し1\n###### 見出し6\n- 項目")).toEqual({
      bullets: ["項目"],
      paragraphs: ["見出し1", "見出し6"],
    });
  });
});

describe("digestDisplay", () => {
  it("maps statuses", () => {
    expect(digestDisplay(view(digest({ body: "x" })), status).kind).toBe("done");
    expect(digestDisplay(view(null), status).kind).toBe("none");
    expect(digestDisplay(view(null), { ...status, digesting: true }).kind).toBe("generating");
    const pending = digestDisplay(view(digest({ status: "pending", attempts: 1, last_error: "ollama down" })), status);
    expect(pending).toEqual({ kind: "pending", error: "ollama down", attempts: 1 });
    expect(digestDisplay(view(digest({ status: "pending" })), { ...status, digesting: true }).kind).toBe("generating");
    const failed = digestDisplay(view(digest({ status: "failed", attempts: 3, last_error: "404" })), status);
    expect(failed).toEqual({ kind: "failed", error: "404", attempts: 3 });
    // 生成済みは digesting 中でもそのまま表示する。
    expect(digestDisplay(view(digest({ body: "x" })), { ...status, digesting: true }).kind).toBe("done");
  });

  it("carries the period item count when there is no digest", () => {
    expect(digestDisplay(view(null, 0), status)).toEqual({ kind: "none", itemCount: 0 });
    expect(digestDisplay(view(null, 12), status)).toEqual({ kind: "none", itemCount: 12 });
  });

  it("shows a requested scope as generating whatever its stored state", () => {
    const pending = digest({ status: "pending", attempts: 1, last_error: "ollama down" });
    const failed = digest({ status: "failed", attempts: 3, last_error: "404" });
    expect(digestDisplay(view(null, 12), status, true)).toEqual({ kind: "generating" });
    expect(digestDisplay(view(null, 0), status, true)).toEqual({ kind: "generating" });
    expect(digestDisplay(view(pending), status, true)).toEqual({ kind: "generating" });
    expect(digestDisplay(view(failed), status, true)).toEqual({ kind: "generating" });
  });

  it("keeps a requested done digest and flags it as regenerating", () => {
    const done = digest({ body: "x" });
    expect(digestDisplay(view(done), status, true)).toEqual({ kind: "done", digest: done, regenerating: true });
    expect(digestDisplay(view(done), status)).toEqual({ kind: "done", digest: done, regenerating: false });
    expect(digestDisplay(view(done), { ...status, digesting: true }, false)).toEqual({ kind: "done", digest: done, regenerating: false });
  });

  it("leaves scopes that were not requested to the global flag", () => {
    const busy = { ...status, digesting: true };
    const failed = digest({ status: "failed", attempts: 3, last_error: "404" });
    // 他のジョブが動いていても、失敗は失敗のまま見せる。
    expect(digestDisplay(view(failed), busy, false)).toEqual({ kind: "failed", error: "404", attempts: 3 });
    expect(digestDisplay(view(failed), status, false)).toEqual({ kind: "failed", error: "404", attempts: 3 });
    expect(digestDisplay(view(null, 12), busy, false)).toEqual({ kind: "generating" });
    expect(digestDisplay(view(digest({ status: "pending" })), busy, false)).toEqual({ kind: "generating" });
    expect(digestDisplay(view(digest({ status: "pending", attempts: 2, last_error: "x" })), status, false))
      .toEqual({ kind: "pending", error: "x", attempts: 2 });
  });
});
