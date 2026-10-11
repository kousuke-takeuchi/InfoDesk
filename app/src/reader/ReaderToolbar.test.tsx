import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it } from "vitest";
import { emitEvent, setupIpc } from "../test/ipc";
import { ReaderToolbar } from "./ReaderToolbar";

describe("ReaderToolbar", () => {
  it("shows the current page and calls the reader commands", async () => {
    const ipc = setupIpc({
      reader_state: () => ({ url: "https://news.example.com/a/b", title: "認知症の新薬" }),
      reader_back: () => null, reader_forward: () => null, reader_reload: () => null,
      reader_open_external: () => null, reader_close: () => null,
    });
    render(<ReaderToolbar />);
    expect(await screen.findByText("認知症の新薬")).toBeInTheDocument();
    expect(screen.getByText("news.example.com")).toBeInTheDocument();
    const user = userEvent.setup();
    for (const [name, cmd] of [["戻る", "reader_back"], ["進む", "reader_forward"], ["再読み込み", "reader_reload"], ["ブラウザで開く", "reader_open_external"], ["閉じる", "reader_close"]] as const) {
      await user.click(screen.getByRole("button", { name }));
      expect(ipc.count(cmd)).toBe(1);
    }
  });

  it("follows page changes", async () => {
    let state = { url: "https://a.example.com/", title: "A" };
    setupIpc({ reader_state: () => state });
    render(<ReaderToolbar />);
    await screen.findByText("A");
    state = { url: "https://b.example.com/x", title: "B" };
    await emitEvent("reader-state");
    expect(await screen.findByText("B")).toBeInTheDocument();
  });
});
