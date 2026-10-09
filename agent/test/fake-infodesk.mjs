#!/usr/bin/env node
// テスト用の偽 infodesk。受け取った引数を JSON で返す。特定の引数では失敗や不正な出力を返す。
const argv = process.argv.slice(2);
if (argv.includes("item") && argv.includes("404")) {
  process.stderr.write("error: item 404 not found\n");
  process.exit(1);
}
if (argv.includes("bad-json")) {
  process.stdout.write("not json");
  process.exit(0);
}
if (argv.includes("sleep")) {
  setTimeout(() => process.stdout.write("{}"), 5000);
} else {
  process.stdout.write(JSON.stringify({ argv }));
}
