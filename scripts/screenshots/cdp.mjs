// usage: node cdp.mjs <devtools-port> <outdir> name|WxH|query ...
// The page is loaded from $BASE (default http://127.0.0.1:4789), which reshoot.sh serves.
import { writeFileSync } from "node:fs";
const [port, outdir, ...jobs] = process.argv.slice(2);
const base = process.env.BASE || "http://127.0.0.1:4789";
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
async function target() {
  for (let i = 0; i < 50; i++) { try { const r = await fetch(`http://127.0.0.1:${port}/json/new?about:blank`, { method: "PUT" }); return await r.json(); } catch { await sleep(200); } }
  throw new Error("no devtools");
}
const t = await target();
const ws = new WebSocket(t.webSocketDebuggerUrl);
await new Promise((r) => (ws.onopen = r));
let id = 0; const pending = new Map(); const logs = [];
ws.onmessage = (m) => { const d = JSON.parse(m.data); if (d.id && pending.has(d.id)) { pending.get(d.id)(d); pending.delete(d.id); } else if (d.method === "Runtime.consoleAPICalled") logs.push(d.params.type + ": " + d.params.args.map((a) => a.value ?? a.description ?? "").join(" ").slice(0, 300)); else if (d.method === "Runtime.exceptionThrown") logs.push("EXC: " + (d.params.exceptionDetails.exception?.description || d.params.exceptionDetails.text).slice(0, 400)); };
const send = (method, params = {}, timeout = 8000) => new Promise((res, rej) => { const i = ++id; pending.set(i, res); ws.send(JSON.stringify({ id: i, method, params })); setTimeout(() => { if (pending.has(i)) { pending.delete(i); rej(new Error("timeout " + method)); } }, timeout); });
await send("Page.enable"); await send("Runtime.enable");
for (const job of jobs) {
  const [name, size, query] = job.split("|"); const [w, h] = size.split("x").map(Number);
  logs.length = 0;
  try {
    await send("Emulation.setDeviceMetricsOverride", { width: w, height: h, deviceScaleFactor: 1, mobile: false });
    await send("Page.navigate", { url: `${base}/${query}` });
    await sleep(Number(process.env.WAIT || 3500));
    const probe = await send("Runtime.evaluate", { expression: process.env.PROBE || "document.body.innerText.length", returnByValue: true, awaitPromise: true }, 8000);
    const shot = await send("Page.captureScreenshot", { format: "png" }, 8000);
    writeFileSync(`${outdir}/${name}.png`, Buffer.from(shot.result.data, "base64"));
    console.log(name, "ok", probe.result.result.value, logs.length ? "LOGS " + JSON.stringify(logs.slice(0, 4)) : "");
  } catch (e) { console.log(name, "FAIL", e.message, JSON.stringify(logs.slice(0, 6))); }
}
ws.close(); process.exit(0);
