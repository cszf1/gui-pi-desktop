// Item 3: newest screenshot per window + pinned evidence in context; every image stored byte for byte and recallable.
import { test } from "node:test";
import assert from "node:assert/strict";
import { mkdtempSync, readFileSync, statSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { ImageStore, MAX_PINS, PINS_ENTRY } from "../src/images.ts";
import { prune } from "../src/context.ts";
import { load, txt, read, IMG } from "./harness.ts";

const shot = (win: string, id: string) => ({ role: "toolResult", toolName: "click",
  content: [{ type: "text", text: "Clicked" }, { type: "image", data: id, mimeType: "image/jpeg" }, { type: "text", text: `[client: screenshot ${id}]` }],
  details: { windowKey: win, images: [id] } });
const imgs = (ms: any[]) => ms.flatMap((m) => (m.content || []).filter((c: any) => c.type === "image").map((c: any) => c.data));

test("store: bytes unchanged, private files, ids continue after a restart, pins max 3", () => {
  const dir = join(mkdtempSync(join(tmpdir(), "img-")), "s");
  const s = new ImageStore(dir);
  const a = s.save(IMG, "image/jpeg", "1:2", "click");
  assert.equal(a, "img-1");
  assert.equal(s.get(a)!.data, IMG);
  assert.deepEqual(readFileSync(join(dir, "img-1.bin")), Buffer.from(IMG, "base64"));
  if (process.platform !== "win32") {
    // Windows has no mode bits: the files sit in the user's own profile, private by its ACL
    assert.equal(statSync(join(dir, "img-1.bin")).mode & 0o777, 0o600);
    assert.equal(statSync(dir).mode & 0o777, 0o700);
  }
  for (let i = 0; i < 4; i++) s.save(IMG, "image/jpeg", "1:2", "click");
  assert.equal(new ImageStore(dir).save(IMG, "image/jpeg", "1:2", "x"), "img-6");
  assert.equal(s.get("../../etc/passwd"), null);
  for (const id of ["img-1", "img-2", "img-3"]) assert.equal(s.pin(id, `ev ${id}`), null);
  assert.match(s.pin("img-4", "one more")!, /at most 3/);
  assert.match(s.pin("img-2", "")!, /label/);
  assert.match(s.pin("img-99", "x")!, /no stored image/);
  assert.ok(s.unpin("img-1"));
  assert.equal(s.pin("img-4", "now fits"), null);
  assert.equal(s.pins.length, MAX_PINS);
  const s2 = new ImageStore(dir);
  assert.ok(s2.restore([{ type: "custom", customType: PINS_ENTRY, data: s.pinsJSON() }]));
  assert.deepEqual(s2.pins.map((p) => p.id), ["img-2", "img-3", "img-4"]);
});

test("context: newest per window, pinned evidence kept, dropped ones name their id", () => {
  const ms = [shot("1:1", "img-1"), shot("1:1", "img-2"), shot("1:1", "img-3"), shot("2:2", "img-4")];
  let out = prune(ms as any);
  assert.deepEqual(imgs(out), ["img-3", "img-4"]);
  assert.match(txt(out[0]), /img-1 removed from context.*recall_image/);
  out = prune(ms as any, { pinned: new Set(["img-1"]) });
  assert.deepEqual(imgs(out), ["img-1", "img-3", "img-4"]);
  // a recall result stays (the newest one only)
  const rec = (id: string) => ({ role: "toolResult", toolName: "recall_image", content: [{ type: "image", data: id }], details: { images: [id] } });
  out = prune([...ms, rec("img-1"), rec("img-2")] as any);
  assert.deepEqual(imgs(out), ["img-3", "img-4", "img-2"]);
});

test("end to end: every image tagged and stored, pin_evidence, recall_image returns the original", async () => {
  const h = await load({ tools: ["get_window_state", "click"], results: { get_window_state: read(42, 7, "s1", true) } });
  const r = await h.pi.call("get_window_state", { pid: 42, window_id: 7, include_screenshot: true });
  assert.match(txt(r), /screenshot img-1 \(window 42:7\)/);
  assert.deepEqual(r.details.images, ["img-1"]);
  assert.equal(r.content.find((c: any) => c.type === "image").data, IMG);
  const p = await h.pi.call("pin_evidence", { id: "img-1", label: "before the edit" });
  assert.match(txt(p), /pinned img-1/);
  assert.deepEqual(h.pi.entries.filter((e) => e.type === PINS_ENTRY).at(-1)!.data.pins, [{ id: "img-1", label: "before the edit" }]);
  const back = await h.pi.call("recall_image", { id: "img-1" });
  assert.equal(back.content.find((c: any) => c.type === "image").data, IMG);
  assert.equal((await h.pi.call("recall_image", { id: "img-9" })).isError, true);
  // the stored copy lives under the session dir
  assert.ok(h.api.images().dir.startsWith(h.dir));
  h.api.mcp.close();
});
