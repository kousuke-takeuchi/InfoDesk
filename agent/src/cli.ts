import { execFile } from "node:child_process";

export class CliError extends Error {}

export type CliRunner = (args: string[], signal?: AbortSignal) => Promise<unknown>;

const MAX_BUFFER = 8 * 1024 * 1024;

export function createCliRunner(command: string[], db: string, timeoutMs = 60_000, maxBuffer = MAX_BUFFER): CliRunner {
  const [bin, ...prefix] = command;
  return (args, signal) =>
    new Promise((resolve, reject) => {
      execFile(
        bin,
        [...prefix, "--db", db, ...args],
        { timeout: timeoutMs, maxBuffer, signal, encoding: "utf8" },
        (err, stdout, stderr) => {
          if (err) {
            const code = (err as NodeJS.ErrnoException).code;
            if (code === "ENOENT") return reject(new CliError(`infodesk CLI not found: ${bin}`));
            if (err.name === "AbortError") return reject(new CliError("infodesk CLI was aborted"));
            if (code === "ERR_CHILD_PROCESS_STDIO_MAXBUFFER") return reject(new CliError("infodesk CLI output was too large"));
            if ((err as { killed?: boolean }).killed) return reject(new CliError("infodesk CLI timed out"));
            const trimmed = stderr.trim();
            const errorLines = trimmed.split("\n").filter((l) => l.startsWith("error:"));
            const last = errorLines.at(-1);
            const msg = last !== undefined ? last.replace(/^error:\s*/, "").trim() : trimmed;
            return reject(new CliError((msg || err.message).slice(0, 500)));
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
