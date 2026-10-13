import { describe, expect, it } from "vitest";
import { matchShortcut, stepSelection, type KeyInfo } from "./shortcuts";
import type { Topic } from "../types";

function key(k: string, over: Partial<KeyInfo> = {}): KeyInfo {
  return { key: k, ctrlKey: false, metaKey: false, altKey: false, shiftKey: false, target: document.body, ...over };
}
const CTX = { inSettings: false, dialogOpen: false };

describe("matchShortcut", () => {
  it.each([
    ["1", "digest"], ["2", "feed"], ["d", "periodDay"], ["w", "periodWeek"], ["m", "periodMonth"],
    ["j", "next"], ["k", "prev"], ["Enter", "open"], ["u", "toggleRead"], ["s", "toggleStar"],
    ["/", "search"], ["[", "sidebarPrev"], ["]", "sidebarNext"], ["0", "selectAll"],
  ])("maps plain %s to %s", (k, action) => {
    expect(matchShortcut(key(k), CTX)).toBe(action);
  });

  it("maps letters typed with Caps Lock on (upper case without shift)", () => {
    expect(matchShortcut(key("J"), CTX)).toBe("next");
    expect(matchShortcut(key("K"), CTX)).toBe("prev");
    expect(matchShortcut(key("U"), CTX)).toBe("toggleRead");
    expect(matchShortcut(key("S"), CTX)).toBe("toggleStar");
    expect(matchShortcut(key("D"), CTX)).toBe("periodDay");
    // Shift 付きの大文字は別のキー (Shift+A 以外は何もしない)。
    expect(matchShortcut(key("J", { shiftKey: true }), CTX)).toBeNull();
    expect(matchShortcut(key("Enter"), CTX)).toBe("open");
  });

  it("maps Shift+A and ? with shift", () => {
    expect(matchShortcut(key("A", { shiftKey: true }), CTX)).toBe("markAllRead");
    expect(matchShortcut(key("?", { shiftKey: true }), CTX)).toBe("help");
    expect(matchShortcut(key("?"), CTX)).toBe("help");
  });

  it("maps Ctrl/Cmd combos", () => {
    for (const m of [{ ctrlKey: true }, { metaKey: true }]) {
      expect(matchShortcut(key("r", m), CTX)).toBe("collect");
      expect(matchShortcut(key(",", m), CTX)).toBe("settings");
      expect(matchShortcut(key("j", m), CTX)).toBe("chat");
    }
  });

  it("ignores plain letters that carry a modifier", () => {
    for (const k of ["1", "d", "w", "m", "j", "k", "u", "s", "/", "[", "]", "0"]) {
      if (k !== "j") {
        expect(matchShortcut(key(k, { ctrlKey: true }), CTX)).toBeNull();
        expect(matchShortcut(key(k, { metaKey: true }), CTX)).toBeNull();
      }
      expect(matchShortcut(key(k, { altKey: true }), CTX)).toBeNull();
    }
    expect(matchShortcut(key("r", { ctrlKey: true, altKey: true }), CTX)).toBeNull();
    expect(matchShortcut(key("a"), CTX)).toBeNull();
    expect(matchShortcut(key("x"), CTX)).toBeNull();
  });

  it("is suppressed in inputs, textareas, selects and contenteditable", () => {
    const ce = document.createElement("div");
    ce.contentEditable = "true";
    Object.defineProperty(ce, "isContentEditable", { value: true });
    for (const target of [document.createElement("input"), document.createElement("textarea"), document.createElement("select"), ce]) {
      expect(matchShortcut(key("j", { target }), CTX)).toBeNull();
      expect(matchShortcut(key("r", { ctrlKey: true, target }), CTX)).toBeNull();
      expect(matchShortcut(key("?", { shiftKey: true, target }), CTX)).toBeNull();
    }
  });

  it("is suppressed during IME composition", () => {
    expect(matchShortcut(key("j", { isComposing: true }), CTX)).toBeNull();
    expect(matchShortcut(key("j", { keyCode: 229 }), CTX)).toBeNull();
    expect(matchShortcut(key("Enter", { isComposing: true }), CTX)).toBeNull();
  });

  it("is suppressed while a dialog is open", () => {
    expect(matchShortcut(key("j"), { inSettings: false, dialogOpen: true })).toBeNull();
    expect(matchShortcut(key("r", { ctrlKey: true }), { inSettings: false, dialogOpen: true })).toBeNull();
    expect(matchShortcut(key("?", { shiftKey: true }), { inSettings: false, dialogOpen: true })).toBeNull();
  });

  it("does not take Enter from a focused button or link", () => {
    expect(matchShortcut(key("Enter", { target: document.createElement("button") }), CTX)).toBeNull();
    expect(matchShortcut(key("Enter", { target: document.createElement("a") }), CTX)).toBeNull();
    expect(matchShortcut(key("j", { target: document.createElement("button") }), CTX)).toBe("next");
  });

  it("limits the settings view to collect, settings, chat and help", () => {
    const ctx = { inSettings: true, dialogOpen: false };
    expect(matchShortcut(key("r", { ctrlKey: true }), ctx)).toBe("collect");
    expect(matchShortcut(key("r", { metaKey: true }), ctx)).toBe("collect");
    expect(matchShortcut(key(",", { metaKey: true }), ctx)).toBe("settings");
    expect(matchShortcut(key("j", { metaKey: true }), ctx)).toBe("chat");
    expect(matchShortcut(key("?", { shiftKey: true }), ctx)).toBe("help");
    for (const k of ["1", "2", "d", "j", "k", "0", "[", "]", "/", "Enter", "u", "s"]) {
      expect(matchShortcut(key(k), ctx)).toBeNull();
    }
    expect(matchShortcut(key("A", { shiftKey: true }), ctx)).toBeNull();
  });
});

describe("stepSelection", () => {
  const topics: Topic[] = [{ id: 5, name: "a", keywords: [] }, { id: 9, name: "b", keywords: [] }];
  it("walks all, categories, then topics, wrapping around", () => {
    expect(stepSelection({ kind: "all" }, topics, 1)).toEqual({ kind: "category", category: "github" });
    expect(stepSelection({ kind: "category", category: "medcare" }, topics, 1)).toEqual({ kind: "topic", topicId: 5 });
    expect(stepSelection({ kind: "topic", topicId: 9 }, topics, 1)).toEqual({ kind: "all" });
    expect(stepSelection({ kind: "all" }, topics, -1)).toEqual({ kind: "topic", topicId: 9 });
    expect(stepSelection({ kind: "category", category: "github" }, topics, -1)).toEqual({ kind: "all" });
    expect(stepSelection({ kind: "all" }, [], -1)).toEqual({ kind: "category", category: "medcare" });
  });
  it("starts from the top when the current topic is gone", () => {
    expect(stepSelection({ kind: "topic", topicId: 77 }, topics, 1)).toEqual({ kind: "category", category: "github" });
  });
});
