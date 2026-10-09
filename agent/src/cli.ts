import { execFile } from "node:child_process";

export class CliError extends Error {}

export type CliRunner = (args: string[], signal?: AbortSignal) => Promise<unknown>;

const MAX_BUFFER = 8 * 1024 * 1024;

export function createCliRunner(command: string[], db: string, timeoutMs = 60_000): CliRunner {
  const [bin, ...prefix] = command;
  return (args, signal) =>
    new Promise((resolve, reject) => {
      execFile(
        bin,
        [...prefix, "--db", db, ...args],
        { timeout: timeoutMs, maxBuffer: MAX_BUFFER, signal, encoding: "utf8" },
        (err, stdout, stderr) => {
          if (err) {
            const code = (err as NodeJS.ErrnoException).code;
            if (code === "ENOENT") return reject(new CliError(`infodesk CLI not found: ${bin}`));
            if (err.name === "AbortError") return reject(new CliError("infodesk CLI was aborted"));
            if ((err as { killed?: boolean }).killed) return reject(new CliError("infodesk CLI timed out"));
            const msg = stderr.trim().replace(/^error:\s*/, "");
            return reject(new CliError(msg || err.message));
          }
          try {
            resolve(JSON.parse(stdout));
          } catch {
            reject(new CliError("infodesk CLI returned invalid JSON"));
          }
        },
      );
    });
}
