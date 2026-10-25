// Item 4: prompt hygiene. Each instruction lives in one place (system prompt, one skill, or the gate's own
// briefing); skills are listed by name + description and loaded on demand, never pasted into the system prompt.
import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync, readdirSync, existsSync } from "node:fs";
import { join, dirname, basename } from "node:path";
import { fileURLToPath } from "node:url";
import { SkillIndex, scanSkills, parseFrontmatter, stripFrontmatter } from "../src/skills.ts";
import { load, txt } from "./harness.ts";

const ROOT = join(dirname(fileURLToPath(import.meta.url)), "../../..");
const SKILLS = join(ROOT, "skills");
const GATE_SKILLS = join(process.env.GUI_GATE_SRC || "/workspace/work/gui-gate", "src/skills.md");

const STOP = new Set("the a an of to and or in on for it its is are be with this that you your by as at from".split(" "));
/** Sentences as content words (5 or more). */
function sentences(md: string): string[][] {
  return stripFrontmatter(md).split(/[.!?;\n|]+/)
    .map((s) => s.toLowerCase().replace(/[^a-z0-9 ]+/g, " ").split(/\s+/).filter((w) => w && !STOP.has(w)))
    .filter((w) => w.length >= 5);
}
const bigrams = (w: string[]) => new Set(w.slice(1).map((x, i) => `${w[i]} ${x}`));
/** Share of the shorter sentence's word pairs found in the other: 0.6+ is the same instruction reworded. */
export function overlap(a: string[], b: string[]): number {
  const x = bigrams(a), y = bigrams(b);
  let n = 0;
  for (const p of x) if (y.has(p)) n++;
  return n / Math.max(1, Math.min(x.size, y.size));
}

test("no instruction is written twice across the system prompt, the skills and the gate briefing", () => {
  const files: [string, string][] = [["prompts/system.md", readFileSync(join(ROOT, "prompts/system.md"), "utf8")]];
  for (const d of readdirSync(SKILLS).sort()) files.push([`skills/${d}`, readFileSync(join(SKILLS, d, "SKILL.md"), "utf8")]);
  if (existsSync(GATE_SKILLS)) files.push(["gui-gate src/skills.md", readFileSync(GATE_SKILLS, "utf8")]);
  const all = files.flatMap(([name, md]) => sentences(md).map((w) => ({ name, w })));
  const dups: string[] = [];
  for (let i = 0; i < all.length; i++)
    for (let j = i + 1; j < all.length; j++)
      if (all[i].name !== all[j].name && overlap(all[i].w, all[j].w) >= 0.6)
        dups.push(`${all[i].name}: "${all[i].w.join(" ")}" ~ ${all[j].name}: "${all[j].w.join(" ")}"`);
  assert.deepEqual(dups, []);
});

test("the duplicate check would have caught the 0.2 prompts (old system.md vs gate-replies)", () => {
  const a = sentences("You can never approve your own action: no tool, argument, code or wording of yours authorises anything, and restating the task does not either.")[0];
  const b = sentences("You can never approve your own action. If a step needs approval, the gate asks the person itself.")[0];
  assert.ok(overlap(a, b) >= 0.6);
  const c = sentences("unknown: it may or may not have happened. Never resend it.")[0] || ["unknown", "may", "may", "not", "have", "happened"];
  assert.ok(overlap(c, sentences("| the step may or may not have happened at all |")[0] || ["step", "may", "may", "not", "have", "happened"]) >= 0.6);
});

test("skills: valid frontmatter, platform and app skills present, system prompt stays short and names no skill body", () => {
  const list = scanSkills(SKILLS);
  const names = list.map((s) => s.name);
  for (const n of ["gui-basics", "gate-replies", "ask-human", "platform-linux", "platform-windows", "platform-macos", "office-libreoffice", "browser"])
    assert.ok(names.includes(n), n);
  for (const s of list) {
    assert.match(s.name, /^[a-z0-9]+(-[a-z0-9]+)*$/);
    assert.ok(s.description.length > 20 && s.description.length <= 1024, s.name);
    assert.equal(parseFrontmatter(readFileSync(s.filePath, "utf8")).name, basename(dirname(s.filePath)));
  }
  const sys = readFileSync(join(ROOT, "prompts/system.md"), "utf8");
  assert.ok(sys.length < 4000, `system prompt is ${sys.length} chars`);
  const idx = new SkillIndex();
  idx.set(list);
  const section = idx.section();
  for (const s of list) {
    assert.ok(section.includes(`- ${s.name}: `));
    const body = stripFrontmatter(readFileSync(s.filePath, "utf8"));
    assert.ok(!section.includes(body.split("\n").slice(-1)[0]), `${s.name} body leaked into the section`);
  }
});

test("load_skill: on demand, only listed skills, never another file", async () => {
  const h = await load({ tools: ["click"] });
  const ev: any = { systemPromptOptions: { sections: {}, skills: scanSkills(SKILLS) } };
  await h.pi.emit("before_agent_start", ev);
  assert.match(ev.systemPromptOptions.sections.gui_skills, /load_skill/);
  assert.ok(!ev.systemPromptOptions.sections.gui_skills.includes("| Reply says |"));
  const r = await h.pi.call("load_skill", { name: "gate-replies" });
  assert.match(txt(r), /\| Reply says \|/);
  assert.ok(!txt(r).startsWith("---"));
  for (const bad of ["../../prompts/system", "/etc/passwd", "gui-basics/../../x"]) {
    const x = await h.pi.call("load_skill", { name: bad });
    assert.equal(x.isError, true);
  }
  h.api.mcp.close();
});
