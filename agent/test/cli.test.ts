import { fileURLToPath } from "node:url";
import { describe, expect, it } from "vitest";
import { CliError, createCliRunner } from "../src/cli";

const FAKE = fileURLToPath(new URL("./fake-infodesk.mjs", import.meta.url));

describe("createCliRunner", () => {
  it("passes --db first and parses stdout as JSON", async () => {
    const run = createCliRunner([FAKE], "/tmp/x.db");
    await expect(run(["items", "--json"])).resolves.toEqual({ argv: ["--db", "/tmp/x.db", "items", "--json"] });
  });

  it("turns a non-zero exit into CliError with the CLI's message", async () => {
    const run = createCliRunner([FAKE], "/tmp/x.db");
    const err = (await run(["item", "404", "--json"]).catch((e) => e)) as Error;
    expect(err).toBeInstanceOf(CliError);
    expect(err.message).toBe("item 404 not found");
  });

  it("rejects output that is not JSON", async () => {
    const run = createCliRunner([FAKE], "/tmp/x.db");
    await expect(run(["bad-json"])).rejects.toThrow("infodesk CLI returned invalid JSON");
  });

  it("times out and can be aborted", async () => {
    const run = createCliRunner([FAKE], "/tmp/x.db", 200);
    await expect(run(["sleep"])).rejects.toThrow(CliError);
    const ac = new AbortController();
    const p = createCliRunner([FAKE], "/tmp/x.db")(["sleep"], ac.signal);
    ac.abort();
    await expect(p).rejects.toThrow(CliError);
  });

  it("reports a missing binary clearly", async () => {
    const run = createCliRunner(["/nonexistent/infodesk"], "/tmp/x.db");
    await expect(run(["items"])).rejects.toThrow("infodesk CLI not found: /nonexistent/infodesk");
  });
});
