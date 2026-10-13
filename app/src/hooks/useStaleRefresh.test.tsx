import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { act, renderHook } from "@testing-library/react";
import { useStaleRefresh } from "./useStaleRefresh";

function setVisibility(state: "visible" | "hidden") {
  Object.defineProperty(document, "visibilityState", { configurable: true, get: () => state });
}

describe("useStaleRefresh", () => {
  beforeEach(() => {
    vi.useFakeTimers();
    vi.setSystemTime(new Date(2026, 9, 13, 23, 30));
    setVisibility("visible");
  });
  afterEach(() => { vi.useRealTimers(); });

  it("refetches once when focus and visibilitychange arrive together", () => {
    const onStale = vi.fn();
    renderHook(() => useStaleRefresh(onStale));
    act(() => {
      window.dispatchEvent(new Event("focus"));
      document.dispatchEvent(new Event("visibilitychange"));
      vi.advanceTimersByTime(500);
    });
    expect(onStale).toHaveBeenCalledTimes(1);
  });

  it("ignores visibilitychange to hidden", () => {
    const onStale = vi.fn();
    renderHook(() => useStaleRefresh(onStale));
    setVisibility("hidden");
    act(() => { document.dispatchEvent(new Event("visibilitychange")); vi.advanceTimersByTime(500); });
    expect(onStale).not.toHaveBeenCalled();
  });

  it("refetches when the local date changes, checked once a minute", () => {
    const onStale = vi.fn();
    renderHook(() => useStaleRefresh(onStale));
    act(() => { vi.advanceTimersByTime(60_000 * 20); });
    expect(onStale).not.toHaveBeenCalled();
    vi.setSystemTime(new Date(2026, 9, 14, 0, 5));
    act(() => { vi.advanceTimersByTime(60_000 + 500); });
    expect(onStale).toHaveBeenCalledTimes(1);
    act(() => { vi.advanceTimersByTime(60_000 * 5); });
    expect(onStale).toHaveBeenCalledTimes(1);
  });

  it("uses the latest callback and stops after unmount", () => {
    const first = vi.fn();
    const second = vi.fn();
    const { rerender, unmount } = renderHook(({ cb }) => useStaleRefresh(cb), { initialProps: { cb: first } });
    rerender({ cb: second });
    act(() => { window.dispatchEvent(new Event("focus")); vi.advanceTimersByTime(500); });
    expect(first).not.toHaveBeenCalled();
    expect(second).toHaveBeenCalledTimes(1);
    unmount();
    act(() => { window.dispatchEvent(new Event("focus")); vi.advanceTimersByTime(60_000 * 3); });
    expect(second).toHaveBeenCalledTimes(1);
  });
});
