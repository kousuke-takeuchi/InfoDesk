import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { ToolCall } from "./ToolCall";

const renderTool = (status: unknown, isError = false) =>
  render(<ToolCall {...({ toolName: "search_articles", args: { query: "q" }, result: undefined, isError, status } as any)} />);

describe("ToolCall", () => {
  it("shows running for a call in progress", () => {
    renderTool({ type: "running" });
    expect(screen.getByText("実行中")).toBeInTheDocument();
  });

  it("shows done for a completed call", () => {
    renderTool({ type: "complete" });
    expect(screen.getByText("完了")).toBeInTheDocument();
  });

  it("shows failed for an errored call", () => {
    renderTool({ type: "complete" }, true);
    expect(screen.getByText("失敗")).toBeInTheDocument();
  });

  it("treats a call that still requires action as failed", () => {
    renderTool({ type: "requires-action", reason: "tool-calls" });
    expect(screen.getByText("失敗")).toBeInTheDocument();
  });

  it("shows interrupted for a cancelled call and failed for other incomplete reasons", () => {
    const { unmount } = renderTool({ type: "incomplete", reason: "cancelled" });
    expect(screen.getByText("中断")).toBeInTheDocument();
    unmount();
    renderTool({ type: "incomplete", reason: "error" });
    expect(screen.getByText("失敗")).toBeInTheDocument();
  });
});
