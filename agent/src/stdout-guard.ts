// 標準出力はアプリが起動の合図 (INFODESK_AGENT_READY) を読むためだけに使う。アプリはその後読むのをやめることがあるので、
// ライブラリ (ollama など) が console.log で書く出力は標準エラーへ回す。main.ts で最初に読み込む。
const toStderr = (...args: unknown[]) => console.error(...args);
console.log = toStderr;
console.info = toStderr;
console.debug = toStderr;
