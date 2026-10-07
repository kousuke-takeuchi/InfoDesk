import { createElement } from "react";
import { describe, expect, it, vi } from "vitest";
import { render } from "@testing-library/react";
import { invoke } from "@tauri-apps/api/core";
import { useTauriEvent } from "../hooks/useTauriEvent";
import { emitEvent, setupIpc } from "./ipc";

function Probe({ name, onFire }: { name: string; onFire: () => void }) {
  useTauriEvent(name, onFire);
  return null;
}

describe("setupIpc", () => {
  it("delivers emitted events to useTauriEvent listeners", async () => {
    setupIpc({});
    const onFire = vi.fn();
    render(createElement(Probe, { name: "items-changed", onFire }));

    await emitEvent("items-changed");
    expect(onFire).toHaveBeenCalledTimes(1);

    await emitEvent("status-changed");
    expect(onFire).toHaveBeenCalledTimes(1);
  });

  it("keeps event plumbing out of the call log", async () => {
    const ipc = setupIpc({});
    render(createElement(Probe, { name: "items-changed", onFire: () => {} }));
    await emitEvent("items-changed");
    expect(ipc.calls).toEqual([]);
    expect(ipc.unmocked).toEqual([]);
  });

  it("records commands that have no handler and still rejects", async () => {
    const ipc = setupIpc({ known: () => 1 });
    await expect(invoke("known")).resolves.toBe(1);
    await expect(invoke("missing_cmd")).rejects.toThrow("unmocked command: missing_cmd");
    expect(ipc.unmocked).toEqual(["missing_cmd"]);
  });

  it("survives a component unmounted before its listen resolves", () => {
    setupIpc({});
    const { unmount } = render(createElement(Probe, { name: "items-changed", onFire: () => {} }));
    unmount();
  });
});
