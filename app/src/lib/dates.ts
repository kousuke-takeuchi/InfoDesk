import type { Period } from "../types";

const WEEKDAYS = ["日", "月", "火", "水", "木", "金", "土"];

export function formatClock(iso: string): string {
  const d = new Date(iso);
  return `${String(d.getHours()).padStart(2, "0")}:${String(d.getMinutes()).padStart(2, "0")}`;
}

export function localDateString(d: Date): string {
  return `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, "0")}-${String(d.getDate()).padStart(2, "0")}`;
}

export function startOfLocalDay(d: Date): Date {
  return new Date(d.getFullYear(), d.getMonth(), d.getDate());
}

export function addDays(d: Date, n: number): Date {
  return new Date(d.getFullYear(), d.getMonth(), d.getDate() + n, d.getHours(), d.getMinutes());
}

/** 期間の開始 (ローカル 0:00)。週は月曜始まり。 */
export function periodStart(period: Period, d: Date): Date {
  const day = startOfLocalDay(d);
  if (period === "day") return day;
  if (period === "week") {
    const offset = (day.getDay() + 6) % 7; // 月曜 = 0
    return new Date(day.getFullYear(), day.getMonth(), day.getDate() - offset);
  }
  return new Date(day.getFullYear(), day.getMonth(), 1);
}

/** 期間の終了 (排他的)。`start` は periodStart の戻り値。 */
export function periodEnd(period: Period, start: Date): Date {
  if (period === "day") return new Date(start.getFullYear(), start.getMonth(), start.getDate() + 1);
  if (period === "week") return new Date(start.getFullYear(), start.getMonth(), start.getDate() + 7);
  return new Date(start.getFullYear(), start.getMonth() + 1, 1);
}

function dateJa(d: Date, withYear: boolean): string {
  const y = withYear ? `${d.getFullYear()}年` : "";
  return `${y}${d.getMonth() + 1}月${d.getDate()}日(${WEEKDAYS[d.getDay()]})`;
}

export function periodHeading(period: Period, d: Date): { title: string; range: string } {
  const start = periodStart(period, d);
  if (period === "day") return { title: "今日のダイジェスト", range: dateJa(start, true) };
  if (period === "week") {
    const last = addDays(periodEnd("week", start), -1);
    return { title: "今週のダイジェスト", range: `${dateJa(start, true)} 〜 ${dateJa(last, false)}` };
  }
  return { title: "今月のダイジェスト", range: `${start.getFullYear()}年${start.getMonth() + 1}月` };
}

export function relativeTime(iso: string, now: Date): string {
  const t = new Date(iso);
  const diffMin = Math.floor((now.getTime() - t.getTime()) / 60000);
  if (diffMin < 1) return "たった今";
  if (diffMin < 60) return `${diffMin}分前`;
  const diffHours = Math.floor(diffMin / 60);
  if (diffHours < 24 && startOfLocalDay(t).getTime() === startOfLocalDay(now).getTime()) return `${diffHours}時間前`;
  const dayDiff = Math.round((startOfLocalDay(now).getTime() - startOfLocalDay(t).getTime()) / 86400000);
  if (dayDiff <= 0) return `${Math.max(diffHours, 1)}時間前`;
  if (dayDiff === 1) return "昨日";
  if (dayDiff < 7) return `${dayDiff}日前`;
  return `${t.getMonth() + 1}月${t.getDate()}日`;
}
