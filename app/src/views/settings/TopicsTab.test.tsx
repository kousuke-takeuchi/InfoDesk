import { describe, expect, it } from "vitest";
import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { TopicsTab } from "./TopicsTab";
import { setupIpc } from "../../test/ipc";
import type { Topic } from "../../types";

describe("TopicsTab", () => {
  it("lists, adds, edits, and deletes topics", async () => {
    let list: Topic[] = [{ id: 1, name: "認知症", keywords: ["認知症", "dementia"] }];
    const ipc = setupIpc({
      list_topics: () => list,
      sidebar_counts: () => ({ total_unread: 0, categories: {}, topics: { "1": 6 } }),
      add_topic: (a) => { const t = { id: 2, name: a.name, keywords: a.keywords }; list = [...list, t]; return t; },
      update_topic: (a) => { list = list.map((t) => (t.id === a.id ? { ...t, name: a.name, keywords: a.keywords } : t)); },
      delete_topic: (a) => { list = list.filter((t) => t.id !== a.id); },
    });
    render(<TopicsTab />);
    // 名前とキーワードチップの両方に「認知症」が出るので 2 件
    expect(await screen.findAllByText("認知症")).toHaveLength(2);
    expect(screen.getByText("dementia")).toBeInTheDocument();
    expect(screen.getByText("6")).toBeInTheDocument();

    await userEvent.click(screen.getByRole("button", { name: "トピックを追加" }));
    await userEvent.type(screen.getByLabelText("名前"), "介護");
    await userEvent.type(screen.getByLabelText("キーワード"), "介護, 介護保険 ,caregiving");
    await userEvent.click(screen.getByRole("button", { name: "保存" }));
    expect(ipc.last("add_topic")).toEqual({ name: "介護", keywords: ["介護", "介護保険", "caregiving"] });
    expect(await screen.findByText("介護保険")).toBeInTheDocument();

    await userEvent.click(screen.getByRole("button", { name: "認知症 を編集" }));
    const kw = screen.getByLabelText("キーワード");
    expect(kw).toHaveValue("認知症, dementia");
    await userEvent.clear(kw);
    await userEvent.type(kw, "認知症, Alzheimer");
    await userEvent.click(screen.getByRole("button", { name: "保存" }));
    expect(ipc.last("update_topic")).toEqual({ id: 1, name: "認知症", keywords: ["認知症", "Alzheimer"] });

    await userEvent.click(screen.getByRole("button", { name: "介護 を削除" }));
    expect(ipc.last("delete_topic")).toEqual({ id: 2 });
    await waitFor(() => expect(screen.queryByText("介護保険")).not.toBeInTheDocument());
  });

  it("refuses an empty name without calling the backend", async () => {
    const ipc = setupIpc({ list_topics: () => [], sidebar_counts: () => ({ total_unread: 0, categories: {}, topics: {} }) });
    render(<TopicsTab />);
    await userEvent.click(await screen.findByRole("button", { name: "トピックを追加" }));
    await userEvent.type(screen.getByLabelText("キーワード"), "x");
    expect(screen.getByRole("button", { name: "保存" })).toBeDisabled();
    expect(ipc.count("add_topic")).toBe(0);
  });
});
