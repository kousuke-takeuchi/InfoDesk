import { build } from "esbuild";

await build({
  entryPoints: ["src/main.ts"],
  outfile: "dist/agent.mjs",
  bundle: true,
  platform: "node",
  target: "node22",
  format: "esm",
  // ESM の出力で、依存が使う require を使えるようにする。
  banner: { js: "import { createRequire as __cr } from 'node:module'; const require = __cr(import.meta.url);" },
  // Agent SDK が CPU ごとに入れる claude 実行ファイルは使わない (設定 claude.bin の CLI を使う)。
  external: ["@anthropic-ai/claude-agent-sdk-*"],
  logLevel: "info",
});
