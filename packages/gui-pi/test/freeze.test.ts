import { test } from "node:test";
import assert from "node:assert/strict";
import { PersonFreeze } from "../src/freeze.ts";

const G = new Set(["click", "type_text"]);

test("nothing runs while a person is answering; a decline skips the rest of the turn", async () => {
  const f = new PersonFreeze();
  let release!: (v: any) => void;
  const ask = f.wrap(() => new Promise((r) => (release = r)));
  const pending = ask({ message: "ok?" } as any);
  assert.match(f.check("click", G)!, /answering/);
  assert.equal(f.check("set_plan_not_gate", G), null);
  release({ action: "decline" });
  await pending;
  assert.match(f.check("type_text", G)!, /skipped/);
  f.newTurn();
  assert.equal(f.check("click", G), null);
});

test("an accepted answer lets the turn go on; a throw counts as declined", async () => {
  const f = new PersonFreeze();
  await f.wrap(async () => ({ action: "accept", content: {} }))({} as any);
  assert.equal(f.check("click", G), null);
  await assert.rejects(f.wrap(async () => { throw new Error("ipc gone"); })({} as any));
  assert.match(f.check("click", G)!, /skipped/);
  assert.equal(f.open, 0);
});
