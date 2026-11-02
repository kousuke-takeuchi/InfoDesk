import { describe, expect, it } from "vitest";
import { editionHeading, editionOf } from "./editions";
import type { Digest } from "../types";

const at = (y: number, m: number, d: number, h = 0, min = 0) => new Date(y, m - 1, d, h, min);
const dg = (made: Date | string, status: Digest["status"] = "done"): Digest => ({
  id: 1, period: "day", period_start: "2026-11-02", scope: { category: "github" }, body: "b", item_ids: [], provider: "p",
  status, attempts: 0, created_at: typeof made === "string" ? made : made.toISOString(), last_error: null,
});
const now = at(2026, 11, 2, 18);
const heading = (start: string, ds: (Digest | null)[]) => editionHeading(editionOf(start, ds)!, now);

describe("editionOf", () => {
  it("names a digest made during the day its evening edition", () => {
    expect(heading("2026-11-02", [dg(at(2026, 11, 2, 17, 3))])).toEqual({ title: "11月2日 夕刊", range: "11月2日 17時3分までの記事" });
    expect(heading("2026-11-02", [dg(at(2026, 11, 2, 17))]).range).toBe("11月2日 17時までの記事");
    expect(heading("2026-11-02", [dg(at(2026, 11, 2, 23, 59))]).range).toBe("11月2日 23時59分までの記事");
  });

  it("names a digest made after the day its next morning edition", () => {
    expect(heading("2026-11-01", [dg(at(2026, 11, 2, 7, 5))])).toEqual({ title: "11月2日 朝刊", range: "11月1日の記事" });
    expect(editionOf("2026-11-01", [dg(at(2026, 11, 2))])?.kind).toBe("morning");
  });

  it("uses the newest finished digest and ignores the rest", () => {
    expect(editionOf("2026-11-02", [dg(at(2026, 11, 2, 17)), dg(at(2026, 11, 3, 7))])?.kind).toBe("morning");
    expect(editionOf("2026-11-02", [dg(at(2026, 11, 2, 17)), dg(at(2026, 11, 3, 7), "pending"), dg("garbage"), null])?.kind).toBe("evening");
  });

  it("has no edition without a finished readable digest", () => {
    expect(editionOf("2026-11-02", [])).toBeNull();
    expect(editionOf("2026-11-02", [null, dg(at(2026, 11, 2, 17), "failed")])).toBeNull();
    expect(editionOf("2026-11-02", [dg("garbage")])).toBeNull();
  });

  it("gives no cutoff for a time before the day", () => {
    expect(heading("2026-11-02", [dg(at(2026, 11, 1, 23))])).toEqual({ title: "11月2日 夕刊", range: "11月2日の記事" });
  });

  it("crosses month and year boundaries", () => {
    expect(heading("2026-10-31", [dg(at(2026, 11, 1, 7))])).toEqual({ title: "11月1日 朝刊", range: "10月31日の記事" });
    expect(editionHeading(editionOf("2026-12-31", [dg(at(2027, 1, 1, 7))])!, at(2027, 1, 5))).toEqual({ title: "1月1日 朝刊", range: "2026年12月31日の記事" });
  });
});
