import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { Sparkline } from "./Sparkline";

describe("Sparkline", () => {
  it("draws a polyline through every value", () => {
    render(<Sparkline ariaLabel="日ごとの件数" values={[1, 0, 3, 2]} />);
    const svg = screen.getByRole("img", { name: "日ごとの件数" });
    const points = svg.querySelector("polyline")!.getAttribute("points")!.trim().split(/\s+/);
    expect(points).toHaveLength(4);
    expect(points.every((p) => !p.includes("NaN"))).toBe(true);
  });

  it("is flat for all zeros and empty for no values", () => {
    const { container, rerender } = render(<Sparkline ariaLabel="x" values={[0, 0, 0]} />);
    const ys = container.querySelector("polyline")!.getAttribute("points")!.trim().split(/\s+/).map((p) => p.split(",")[1]);
    expect(new Set(ys).size).toBe(1);
    rerender(<Sparkline ariaLabel="x" values={[]} />);
    expect(container.querySelector("polyline")).toBeNull();
  });
});
