import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it } from "vitest";
import { ToolCall } from "./ToolCall";
import { WorkCollapsedContext } from "./WorkSummary";

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

  it("opens the input and result details", async () => {
    render(<ToolCall {...({ toolName: "search_articles", args: { query: "認知症" }, result: { items: [{ id: 1, title: "記事A" }], total: 1 }, isError: false, status: { type: "complete" } } as any)} />);
    const row = screen.getByRole("button", { name: /記事検索/ });
    expect(row).toHaveAttribute("aria-expanded", "false");
    await userEvent.setup().click(row);
    expect(row).toHaveAttribute("aria-expanded", "true");
    expect(screen.getByText("記事A")).toBeInTheDocument();
  });

  it("shows the error text of a failed call", async () => {
    render(<ToolCall {...({ toolName: "search_articles", args: {}, result: { error: "接続できません" }, isError: true, status: { type: "complete" } } as any)} />);
    await userEvent.setup().click(screen.getByRole("button", { name: /記事検索/ }));
    expect(screen.getByText("接続できません")).toBeInTheDocument();
  });

  it("renders a plain row when there are no details to open", () => {
    render(<ToolCall {...({ toolName: "get_article", args: {}, result: undefined, isError: false, status: { type: "complete" } } as any)} />);
    expect(screen.queryByRole("button")).toBeNull();
    const row = screen.getByText("記事を読む").closest(".tool-call");
    expect(row).not.toBeNull();
    expect(row).not.toHaveAttribute("aria-expanded");
    expect(screen.getByText("完了")).toBeInTheDocument();
  });

  it("renders nothing for plan updates", () => {
    const { container } = render(<ToolCall {...({ toolName: "update_plan", args: { items: [] }, result: undefined, isError: false, status: { type: "complete" } } as any)} />);
    expect(container).toBeEmptyDOMElement();
    const running = render(<ToolCall {...({ toolName: "update_plan", args: {}, result: undefined, isError: false, status: { type: "running" } } as any)} />);
    expect(running.container).toBeEmptyDOMElement();
  });

  it("renders nothing while the work is collapsed", () => {
    const { container } = render(
      <WorkCollapsedContext.Provider value={true}>
        <ToolCall {...({ toolName: "search_articles", args: {}, result: undefined, isError: false, status: { type: "complete" } } as any)} />
      </WorkCollapsedContext.Provider>,
    );
    expect(container).toBeEmptyDOMElement();
  });
});
