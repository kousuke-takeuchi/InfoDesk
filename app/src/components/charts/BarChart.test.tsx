import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { BarChart } from "./BarChart";

describe("BarChart", () => {
  it("draws one bar per row with the value as text", () => {
    render(<BarChart ariaLabel="カテゴリー別" rows={[{ label: "医療介護", value: 14 }, { label: "GitHub", value: 7 }, { label: "arXiv", value: 0 }]} />);
    const fig = screen.getByRole("group", { name: "カテゴリー別" });
    expect(fig).toBeInTheDocument();
    expect(screen.getByText("医療介護")).toBeInTheDocument();
    expect(screen.getByText("14")).toBeInTheDocument();
    const bars = document.querySelectorAll("[data-bar]");
    expect(bars).toHaveLength(3);
    expect(Number(bars[0].getAttribute("data-ratio"))).toBe(1);
    expect(Number(bars[1].getAttribute("data-ratio"))).toBe(0.5);
    expect(Number(bars[2].getAttribute("data-ratio"))).toBe(0);
  });

  it("handles all zeros without NaN", () => {
    render(<BarChart ariaLabel="x" rows={[{ label: "a", value: 0 }]} />);
    expect(document.querySelector("[data-bar]")!.getAttribute("data-ratio")).toBe("0");
  });
});
