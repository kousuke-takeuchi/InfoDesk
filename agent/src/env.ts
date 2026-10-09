export interface AgentEnv {
  /** 実行ファイルと前置きの引数。例: ["/Applications/InfoDesk.app/Contents/MacOS/infodesk-app", "--cli"] */
  cli: string[];
  db: string;
  token: string;
  port: number;
}

function required(env: NodeJS.ProcessEnv, name: string): string {
  const v = env[name];
  if (!v) throw new Error(`missing environment variable: ${name}`);
  return v;
}

export function readEnv(env: NodeJS.ProcessEnv = process.env): AgentEnv {
  const port = Number(env.INFODESK_AGENT_PORT ?? "0");
  return {
    cli: [required(env, "INFODESK_CLI"), ...(env.INFODESK_CLI_ARG ? [env.INFODESK_CLI_ARG] : [])],
    db: required(env, "INFODESK_DB"),
    token: required(env, "INFODESK_AGENT_TOKEN"),
    port: Number.isInteger(port) && port >= 0 ? port : 0,
  };
}
