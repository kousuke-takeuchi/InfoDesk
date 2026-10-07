import { act } from "@testing-library/react";
import { emit } from "@tauri-apps/api/event";
import { mockIPC } from "@tauri-apps/api/mocks";

export type IpcHandlers = Record<string, (args: any) => unknown>;

export interface IpcLog {
  calls: { cmd: string; args: any }[];
  /** ハンドラ未登録のまま呼ばれたコマンド名（呼ばれた順） */
  unmocked: string[];
  /** 指定コマンドの最後の引数 */
  last(cmd: string): any;
  count(cmd: string): number;
}

/**
 * Tauri の IPC をモックする。未登録のコマンドは例外にして unmocked に記録し、テストが取りこぼしを検出できるようにする。
 * イベントの listen/emit/unlisten は Tauri のモックが処理するので、calls には現れない。
 */
export function setupIpc(handlers: IpcHandlers): IpcLog {
  const calls: { cmd: string; args: any }[] = [];
  const unmocked: string[] = [];
  mockIPC(async (cmd, args) => {
    calls.push({ cmd, args });
    const handler = handlers[cmd];
    if (!handler) {
      unmocked.push(cmd);
      throw new Error(`unmocked command: ${cmd}`);
    }
    return handler(args);
  }, { shouldMockEvents: true });
  return {
    calls,
    unmocked,
    last: (cmd) => [...calls].reverse().find((c) => c.cmd === cmd)?.args,
    count: (cmd) => calls.filter((c) => c.cmd === cmd).length,
  };
}

/**
 * Tauri のイベントを発火し、購読中のリスナーへ配送されるまで待つ。
 * setupIpc の後に呼ぶ。リスナーが起こした再取得の完了までは待たないので、結果は waitFor / findBy で確認する。
 */
export async function emitEvent(name: string): Promise<void> {
  await act(async () => {
    // マウント直後の listen が登録を終えるのを待ってから発火する
    await new Promise((r) => setTimeout(r, 0));
    await emit(name);
  });
}
