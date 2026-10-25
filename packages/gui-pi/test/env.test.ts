import { test } from "node:test";
import assert from "node:assert/strict";
import { createServer } from "node:net";
import { mkdtempSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { gateEnv, gateArgs, fetchGateEnv } from "../src/env.ts";

test("the gate never gets Pi's model key, the person token or the config switch", () => {
  const env = { PATH: "/usr/bin", HOME: "/h", DISPLAY: ":0", LC_ALL: "C", GUI_PI_MAIN_KEY: "sk-main", GUI_PI_HUMAN_TOKEN: "tok",
                GUI_GATE_ALLOW_CONFIG: "1", OPENAI_API_KEY: "sk-other", GUI_GATE_097: "1", RANDOM_VAR: "x" };
  const e = gateEnv(env, { GUI_GATE_REVIEWER_KEY: "sk-rev", GUI_GATE_ALLOW_CONFIG: "1", GUI_PI_MAIN_KEY: "sk-x", PATH: "/evil" });
  assert.equal(e.GUI_GATE_REVIEWER_KEY, "sk-rev");
  assert.equal(e.GUI_GATE_097, "1");
  assert.equal(e.PATH, "/usr/bin");
  for (const k of ["GUI_PI_MAIN_KEY", "GUI_PI_HUMAN_TOKEN", "GUI_GATE_ALLOW_CONFIG", "OPENAI_API_KEY", "RANDOM_VAR"]) assert.equal(e[k], undefined, k);
  assert.ok(!JSON.stringify(e).includes("sk-main") && !JSON.stringify(e).includes("sk-other"));
});

test("--allow-config is never passed to the gate", () => {
  const a = gateArgs({ GUI_PI_GATE_ARGS: JSON.stringify(["--allow-config", "--lambda", "0.3", "--allow-config=1"]) });
  assert.ok(!a.some((x) => x.startsWith("--allow-config")));
  assert.ok(a.includes("--lambda"));
});

test("gate settings come from the shell's IPC once, with the token", async () => {
  const path = join(mkdtempSync(join(tmpdir(), "genv-")), "h.sock");
  const seen: any[] = [];
  const srv = createServer((sock) => {
    sock.on("data", (d) => {
      const req = JSON.parse(String(d).trim());
      seen.push(req);
      sock.write(JSON.stringify(req.token === "tok" && seen.length === 1 ? { env: { GUI_GATE_REVIEWER_KEY: "sk-rev", N: 3 } } : { env: {} }) + "\n");
    });
  });
  await new Promise<void>((r) => srv.listen(path, () => r()));
  assert.deepEqual(await fetchGateEnv(path, "tok"), { GUI_GATE_REVIEWER_KEY: "sk-rev" });
  assert.deepEqual(await fetchGateEnv(path, "tok"), {});
  assert.equal(seen[0].op, "gate_env");
  assert.deepEqual(await fetchGateEnv(undefined, "tok"), {});
  srv.close();
});
