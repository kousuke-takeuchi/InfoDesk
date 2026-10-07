import { mockIPC } from "@tauri-apps/api/mocks";

export type IpcHandlers = Record<string, (args: any) => unknown>;

export interface IpcLog {
  calls: { cmd: string; args: any }[];
  /** 指定コマンドの最後の引数 */
  last(cmd: string): any;
  count(cmd: string): number;
}

/**
 * Tauri の IPC をモックする。未登録のコマンドは例外にして、テストが取りこぼしを検出できるようにする。
 * イベントの listen/unlisten は常に成功させる。
 */
export function setupIpc(handlers: IpcHandlers): IpcLog {
  const calls: { cmd: string; args: any }[] = [];
  mockIPC(async (cmd, args) => {
    if (cmd === "plugin:event|listen") return 1;
    if (cmd === "plugin:event|unlisten") return undefined;
    calls.push({ cmd, args });
    const handler = handlers[cmd];
    if (!handler) throw new Error(`unmocked command: ${cmd}`);
    return handler(args);
  });
  return {
    calls,
    last: (cmd) => [...calls].reverse().find((c) => c.cmd === cmd)?.args,
    count: (cmd) => calls.filter((c) => c.cmd === cmd).length,
  };
}
