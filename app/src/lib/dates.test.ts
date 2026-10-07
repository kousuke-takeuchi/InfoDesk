import { describe, expect, it } from "vitest";
import { addDays, localDateString, periodEnd, periodHeading, periodStart, relativeTime, startOfLocalDay } from "./dates";

const at = (s: string) => new Date(s);

describe("dates", () => {
  it("uses local dates at the boundaries", () => {
    // Review Focus 5: 23:30 ローカルは同じ日、翌 0:30 は次の日。
    const late = new Date(2026, 9, 6, 23, 30);
    const early = new Date(2026, 9, 7, 0, 30);
    expect(localDateString(late)).toBe("2026-10-06");
    expect(localDateString(early)).toBe("2026-10-07");
    expect(startOfLocalDay(late).getHours()).toBe(0);
    expect(localDateString(startOfLocalDay(early))).toBe("2026-10-07");
    expect(localDateString(addDays(early, -7))).toBe("2026-09-30");
  });

  it("computes period starts and ends with Monday weeks", () => {
    const mon = new Date(2026, 9, 5, 0, 30); // 月曜 0:30
    const sun = new Date(2026, 9, 11, 12, 0);
    expect(localDateString(periodStart("day", mon))).toBe("2026-10-05");
    expect(localDateString(periodStart("week", mon))).toBe("2026-10-05");
    expect(localDateString(periodStart("week", sun))).toBe("2026-10-05");
    expect(localDateString(periodStart("month", sun))).toBe("2026-10-01");
    expect(localDateString(periodEnd("week", periodStart("week", sun)))).toBe("2026-10-12");
    expect(localDateString(periodEnd("month", new Date(2026, 11, 1)))).toBe("2027-01-01");
  });

  it("formats headings", () => {
    const d = new Date(2026, 9, 6, 9, 0);
    expect(periodHeading("day", d)).toEqual({ title: "今日のダイジェスト", range: "2026年10月6日(火)" });
    expect(periodHeading("week", d)).toEqual({ title: "今週のダイジェスト", range: "2026年10月5日(月) 〜 10月11日(日)" });
    expect(periodHeading("month", d)).toEqual({ title: "今月のダイジェスト", range: "2026年10月" });
  });

  it("formats relative time", () => {
    const now = new Date(2026, 9, 6, 12, 0);
    expect(relativeTime(new Date(2026, 9, 6, 11, 59, 40).toISOString(), now)).toBe("たった今");
    expect(relativeTime(new Date(2026, 9, 6, 11, 30).toISOString(), now)).toBe("30分前");
    expect(relativeTime(new Date(2026, 9, 6, 9, 0).toISOString(), now)).toBe("3時間前");
    expect(relativeTime(new Date(2026, 9, 5, 20, 0).toISOString(), now)).toBe("昨日");
    expect(relativeTime(new Date(2026, 9, 3, 20, 0).toISOString(), now)).toBe("3日前");
    expect(relativeTime(new Date(2026, 8, 20, 20, 0).toISOString(), now)).toBe("9月20日");
    expect(relativeTime(at("2026-10-06T00:00:00Z").toISOString(), now)).toMatch(/前|昨日|今/);
  });
});
