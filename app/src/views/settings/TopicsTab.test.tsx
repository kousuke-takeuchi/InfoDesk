import { describe, expect, it } from "vitest";
import { act, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { TopicsTab } from "./TopicsTab";
import { EVENTS } from "../../api";
import { emitEvent, setupIpc } from "../../test/ipc";
import type { Topic } from "../../types";

const COUNTS = { total_unread: 0, categories: {}, topics: {} };

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
    expect(ipc.count("delete_topic")).toBe(0);
    await userEvent.click(screen.getByRole("button", { name: "削除する" }));
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

  it("asks before deleting and warns that digests go too", async () => {
    let list: Topic[] = [
      { id: 1, name: "認知症", keywords: ["dementia"] },
      { id: 2, name: "介護", keywords: ["caregiving"] },
    ];
    const ipc = setupIpc({
      list_topics: () => list,
      sidebar_counts: () => COUNTS,
      delete_topic: (a) => { list = list.filter((t) => t.id !== a.id); },
    });
    render(<TopicsTab />);
    await screen.findByText("認知症");
    const rowOf = (name: string) => within(screen.getByText(name).closest(".table-row") as HTMLElement);
    const CONFIRM = "削除しますか? このトピックの要約も削除されます。";

    await userEvent.click(screen.getByRole("button", { name: "認知症 を削除" }));
    expect(rowOf("認知症").getByText(CONFIRM)).toBeInTheDocument();
    await userEvent.click(screen.getByRole("button", { name: "キャンセル" }));
    expect(screen.queryByText(CONFIRM)).not.toBeInTheDocument();
    expect(ipc.count("delete_topic")).toBe(0);

    await userEvent.click(screen.getByRole("button", { name: "認知症 を削除" }));
    await userEvent.click(screen.getByRole("button", { name: "介護 を削除" }));
    expect(screen.getAllByText(CONFIRM)).toHaveLength(1);
    expect(rowOf("介護").getByText(CONFIRM)).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "認知症 を削除" })).toBeInTheDocument();

    await userEvent.click(screen.getByRole("button", { name: "削除する" }));
    expect(ipc.last("delete_topic")).toEqual({ id: 2 });
    expect(ipc.count("delete_topic")).toBe(1);
    await waitFor(() => expect(screen.queryByText("介護")).not.toBeInTheDocument());
    expect(screen.getByText("認知症")).toBeInTheDocument();
  });

  it("requires a keyword, removes duplicates, and explains keyword edits", async () => {
    let list: Topic[] = [{ id: 1, name: "認知症", keywords: ["dementia"] }];
    const HINT = "キーワードを変えると、このトピックの要約は作り直しになります。";
    const ipc = setupIpc({
      list_topics: () => list,
      sidebar_counts: () => COUNTS,
      add_topic: (a) => { const t = { id: 2, name: a.name, keywords: a.keywords }; list = [...list, t]; return t; },
    });
    render(<TopicsTab />);
    await screen.findByText("認知症");

    await userEvent.click(screen.getByRole("button", { name: "トピックを追加" }));
    expect(screen.queryByText(HINT)).not.toBeInTheDocument();
    await userEvent.type(screen.getByLabelText("名前"), "介護");
    const save = screen.getByRole("button", { name: "保存" });
    expect(save).toBeDisabled();
    await userEvent.type(screen.getByLabelText("キーワード"), " , 、 ");
    expect(save).toBeDisabled();
    expect(ipc.count("add_topic")).toBe(0);

    await userEvent.clear(screen.getByLabelText("キーワード"));
    await userEvent.type(screen.getByLabelText("キーワード"), "介護, caregiving, 介護 、Caregiving,caregiving");
    expect(save).toBeEnabled();
    await userEvent.click(save);
    expect(ipc.last("add_topic")).toEqual({ name: "介護", keywords: ["介護", "caregiving", "Caregiving"] });
    await screen.findByText("Caregiving");

    await userEvent.click(screen.getByRole("button", { name: "認知症 を編集" }));
    expect(screen.getByText(HINT)).toBeInTheDocument();
  });

  it("renders the edit card immediately after the topic being edited and the add card after the list", async () => {
    const list: Topic[] = [
      { id: 1, name: "認知症", keywords: ["dementia"] },
      { id: 2, name: "介護", keywords: ["caregiving"] },
      { id: 3, name: "創薬", keywords: ["drug discovery"] },
    ];
    setupIpc({ list_topics: () => list, sidebar_counts: () => COUNTS });
    render(<TopicsTab />);
    await screen.findByText("認知症");
    const rowEl = (name: string) => screen.getByText(name).closest(".table-row") as HTMLElement;
    const editCard = () => screen.getByText("トピックを編集").closest(".card");

    await userEvent.click(screen.getByRole("button", { name: "認知症 を編集" }));
    expect(rowEl("認知症").nextElementSibling).toBe(editCard());
    expect(editCard()?.nextElementSibling).toBe(rowEl("介護"));

    await userEvent.click(screen.getByRole("button", { name: "介護 を編集" }));
    expect(screen.getAllByText("トピックを編集")).toHaveLength(1);
    expect(rowEl("介護").nextElementSibling).toBe(editCard());
    expect(editCard()?.nextElementSibling).toBe(rowEl("創薬"));
    expect(screen.getByLabelText("名前")).toHaveValue("介護");

    // 新規追加のカードは行の間ではなく一覧の末尾 (最後の行の直後) に出る
    await userEvent.click(screen.getByRole("button", { name: "トピックを追加" }));
    expect(screen.queryByText("トピックを編集")).not.toBeInTheDocument();
    const addCard = screen.getByRole("heading", { name: "トピックを追加" }).closest(".card");
    expect(addCard).toBeVisible();
    expect(rowEl("創薬").nextElementSibling).toBe(addCard);
    expect(addCard?.nextElementSibling).toBeNull();
    expect(rowEl("認知症").nextElementSibling).toBe(rowEl("介護"));
    expect(screen.getByLabelText("名前")).toHaveValue("");
  });

  it("sends one request when 保存 is clicked twice", async () => {
    let finish: () => void = () => {};
    const ipc = setupIpc({
      list_topics: () => [],
      sidebar_counts: () => COUNTS,
      add_topic: (a) => new Promise<Topic>((r) => { finish = () => r({ id: 2, name: a.name, keywords: a.keywords }); }),
    });
    render(<TopicsTab />);
    await userEvent.click(await screen.findByRole("button", { name: "トピックを追加" }));
    await userEvent.type(screen.getByLabelText("名前"), "介護");
    await userEvent.type(screen.getByLabelText("キーワード"), "介護");
    const save = screen.getByRole("button", { name: "保存" });
    await userEvent.click(save);
    expect(save).toBeDisabled();
    await userEvent.click(save);
    expect(ipc.count("add_topic")).toBe(1);
    await act(async () => { finish(); });
    await waitFor(() => expect(screen.queryByLabelText("名前")).not.toBeInTheDocument());
    expect(ipc.count("add_topic")).toBe(1);
  });
  it("reloads the counts on items-changed and keeps an unsaved edit, a confirmation, and the action error", async () => {
    const list: Topic[] = [
      { id: 1, name: "認知症", keywords: ["dementia"] },
      { id: 2, name: "介護", keywords: ["caregiving"] },
    ];
    let unread = 6;
    const ipc = setupIpc({
      list_topics: () => list,
      sidebar_counts: () => ({ total_unread: 0, categories: {}, topics: { "1": unread } }),
      update_topic: () => { throw { message: "database is locked" }; },
    });
    render(<TopicsTab />);
    expect(await screen.findByText("6")).toBeInTheDocument();

    await userEvent.click(screen.getByRole("button", { name: "認知症 を編集" }));
    await userEvent.click(screen.getByRole("button", { name: "保存" }));
    expect(await screen.findByRole("alert")).toHaveTextContent("database is locked");
    await userEvent.clear(screen.getByLabelText("名前"));
    await userEvent.type(screen.getByLabelText("名前"), "認知症 未保存");
    await userEvent.click(screen.getByRole("button", { name: "介護 を削除" }));
    expect(screen.getByText(/削除しますか/)).toBeInTheDocument();

    unread = 9;
    await emitEvent(EVENTS.items);
    expect(await screen.findByText("9")).toBeInTheDocument();
    expect(ipc.count("list_topics")).toBe(2);
    expect(ipc.count("sidebar_counts")).toBe(2);
    expect(screen.getByLabelText("名前")).toHaveValue("認知症 未保存");
    expect(screen.getByText(/削除しますか/)).toBeInTheDocument();
    expect(screen.getByRole("alert")).toHaveTextContent("database is locked");
  });
});
