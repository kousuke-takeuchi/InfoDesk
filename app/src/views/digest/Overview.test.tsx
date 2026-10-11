import { render, screen, within } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import type { DigestStats } from "../../types";
import { Overview, trend } from "./Overview";

const cat = (category: string, count: number, daily: number[] = []) => ({ category, count, daily }) as DigestStats["categories"][number];

const WEEK: DigestStats = {
  period: "week", period_start: "2026-10-05", total: 21,
  categories: [cat("github", 5, [1, 0, 1, 1, 0, 2, 0]), cat("medcare", 14, [2, 2, 2, 2, 2, 2, 2]), cat("arxiv", 2, [0, 0, 0, 0, 0, 2, 0])],
  topics: [
    { topic_id: 1, name: "認知症", count: 6, prev_count: 2 },
    { topic_id: 2, name: "介護", count: 1, prev_count: 4 },
    { topic_id: 3, name: "ローカルLLM", count: 3, prev_count: 3 },
  ],
};

describe("Overview", () => {
  it("shows totals, category bars with sparklines for a week, and topic trends", () => {
    render(<Overview stats={WEEK} error={null} />);
    expect(screen.getByText("21件")).toBeInTheDocument();
    expect(screen.getByRole("group", { name: /カテゴリー別の記事数/ })).toBeInTheDocument();
    expect(screen.getAllByRole("img", { name: /日ごとの件数/ })).toHaveLength(3);
    const topics = screen.getByRole("list", { name: "話題の強さ" });
    const row = (name: string) => within(topics).getByText(name).closest("li")!;
    expect(row("認知症")).toHaveAttribute("data-trend", "up");
    expect(row("認知症")).toHaveTextContent("6件");
    expect(row("認知症")).toHaveTextContent("+4");
    expect(row("介護")).toHaveAttribute("data-trend", "down");
    expect(row("介護")).toHaveTextContent("-3");
    expect(row("ローカルLLM")).toHaveAttribute("data-trend", "flat");
  });

  it("omits sparklines for a day and says when the period is empty", () => {
    const { rerender } = render(<Overview stats={{ ...WEEK, period: "day", categories: WEEK.categories.map((c) => ({ ...c, daily: [] })) }} error={null} />);
    expect(screen.queryAllByRole("img", { name: /日ごとの件数/ })).toHaveLength(0);
    rerender(<Overview stats={{ ...WEEK, total: 0, categories: [], topics: [] }} error={null} />);
    expect(screen.getByText("この期間に記事がありません")).toBeInTheDocument();
  });

  it("classifies trends", () => {
    expect(trend(3, 1)).toBe("up");
    expect(trend(1, 3)).toBe("down");
    expect(trend(2, 2)).toBe("flat");
    expect(trend(0, 0)).toBe("flat");
  });

  it("renders nothing while loading and an error line on failure", () => {
    const { container, rerender } = render(<Overview stats={null} error={null} />);
    expect(container).toBeEmptyDOMElement();
    rerender(<Overview stats={null} error="boom" />);
    expect(screen.getByRole("alert")).toHaveTextContent("boom");
  });
});
