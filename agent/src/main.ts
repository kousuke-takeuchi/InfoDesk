import "./stdout-guard";
import { createCliRunner } from "./cli";
import { streamChat } from "./chat";
import { readEnv } from "./env";
import { defaultModels, type Settings } from "./models";
import { createAgentServer } from "./server";

const ORIGINS = ["tauri://localhost", "http://localhost:1420"];

async function main() {
  if (process.argv.includes("--check")) {
    process.stdout.write("ok\n");
    return;
  }
  const env = readEnv();
  const run = createCliRunner(env.cli, env.db);
  const loadSettings = async (): Promise<Settings> => ((await run(["config", "list", "--json"])) as { settings: Settings }).settings;
  const server = createAgentServer({
    token: env.token,
    allowedOrigins: ORIGINS,
    chat: (req, res, signal) => streamChat(req, res, { run, loadSettings, models: defaultModels, now: () => new Date() }, signal),
  });
  server.listen(env.port, "127.0.0.1", () => {
    const address = server.address();
    const port = typeof address === "object" && address ? address.port : env.port;
    process.stdout.write(`INFODESK_AGENT_READY ${JSON.stringify({ port })}\n`);
  });
  // アプリが終了または異常終了したら標準入力が閉じる。そのとき一緒に終わる。
  process.stdin.on("end", () => process.exit(0));
  process.stdin.resume();
  process.on("SIGTERM", () => server.close(() => process.exit(0)));
}

// 取りこぼした reject でサイドカーごと落ちないようにする (本来は各所で処理する。これは保険)。
process.on("unhandledRejection", (reason) => {
  process.stderr.write(`unhandled rejection: ${reason instanceof Error ? reason.stack ?? reason.message : String(reason)}\n`);
});

main().catch((e) => {
  process.stderr.write(`${e instanceof Error ? e.message : String(e)}\n`);
  process.exit(1);
});
