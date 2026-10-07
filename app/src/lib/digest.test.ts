import { describe, expect, it } from "vitest";
import { digestDisplay, parseDigestBody } from "./digest";
import type { AppStatus, Digest, DigestView } from "../types";

const status: AppStatus = { collecting: false, summarizing: false, digesting: false, last_collect_at: null, next_collect_at: null, last_error: null };
const digest = (over: Partial<Digest>): Digest => ({
  id: 1, period: "day", period_start: "2026-10-06", scope: { category: "github" }, body: "", item_ids: [],
  provider: "ollama:gemma4:26b", status: "done", attempts: 0, created_at: "2026-10-06T00:00:00Z", last_error: null, ...over,
});
const view = (d: Digest | null): DigestView => ({ scope: { category: "github" }, label: "GitHub", digest: d });

describe("parseDigestBody", () => {
  it("extracts bullets and keeps the rest as paragraphs", () => {
    const body = "- 一つ目。\n* 二つ目。\n・三つ目\n• 四つ目\n1. 五つ目\n\n全体として伸びている。";
    expect(parseDigestBody(body)).toEqual({
      bullets: ["一つ目。", "二つ目。", "三つ目", "四つ目", "五つ目"],
      paragraphs: ["全体として伸びている。"],
    });
    expect(parseDigestBody("")).toEqual({ bullets: [], paragraphs: [] });
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
});
