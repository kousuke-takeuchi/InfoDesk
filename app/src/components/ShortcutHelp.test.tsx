import { describe, expect, it, vi } from "vitest";
import { fireEvent, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { ShortcutHelp } from "./ShortcutHelp";

describe("ShortcutHelp", () => {
  it("is a labelled modal dialog listing the keys in Japanese", () => {
    render(<ShortcutHelp onClose={() => {}} />);
    const dialog = screen.getByRole("dialog", { name: "ショートカット" });
    expect(dialog).toHaveAttribute("aria-modal", "true");
    for (const text of ["今すぐ収集", "ダイジェスト", "フィード", "次の記事", "前の記事", "記事を開く", "既読", "スター", "検索", "すべて既読", "チャット", "設定", "ヘルプ"]) {
      expect(dialog).toHaveTextContent(text);
    }
    for (const k of ["⌘R", "⌘,", "⌘J", "Shift+A", "?"]) expect(dialog).toHaveTextContent(k);
  });

  it("closes with Esc and with the close button", async () => {
    const onClose = vi.fn();
    render(<ShortcutHelp onClose={onClose} />);
    fireEvent.keyDown(window, { key: "Escape" });
    expect(onClose).toHaveBeenCalledTimes(1);
    await userEvent.click(screen.getByRole("button", { name: "閉じる" }));
    expect(onClose).toHaveBeenCalledTimes(2);
  });

  it("keeps Tab and Shift+Tab inside the dialog", async () => {
    render(<><button>外のボタン</button><ShortcutHelp onClose={() => {}} /></>);
    const close = screen.getByRole("button", { name: "閉じる" });
    expect(close).toHaveFocus();
    await userEvent.tab();
    expect(close).toHaveFocus();
    await userEvent.tab({ shift: true });
    expect(close).toHaveFocus();
  });

  it("returns focus to the element that had it before opening", () => {
    const { rerender } = render(<><button>前のボタン</button></>);
    const before = screen.getByRole("button", { name: "前のボタン" });
    before.focus();
    rerender(<><button>前のボタン</button><ShortcutHelp onClose={() => {}} /></>);
    expect(screen.getByRole("button", { name: "閉じる" })).toHaveFocus();
    rerender(<><button>前のボタン</button></>);
    expect(before).toHaveFocus();
  });
});
