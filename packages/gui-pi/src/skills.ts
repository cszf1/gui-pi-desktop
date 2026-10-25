// Skills on demand. Pi lists skills in the system prompt only when its own read/bash tools are active, and the
// desktop agent has neither (file access goes through the gate). So the extension lists the skills Pi loaded
// (--skill, name + description only) in one prompt section and gives the model `load_skill {name}`, which returns
// that skill's SKILL.md body - only for skills in that list, never any other file.
import { readFileSync, readdirSync, existsSync } from "node:fs";
import { join } from "node:path";

export const SECTION = "gui_skills";
export type SkillRef = { name: string; description: string; filePath: string };

export function stripFrontmatter(md: string): string {
  const m = md.match(/^---\r?\n[\s\S]*?\r?\n---\r?\n?/);
  return (m ? md.slice(m[0].length) : md).trim();
}

export function parseFrontmatter(md: string): { name?: string; description?: string } {
  const m = md.match(/^---\r?\n([\s\S]*?)\r?\n---/);
  const out: any = {};
  if (!m) return out;
  for (const line of m[1].split(/\r?\n/)) {
    const kv = line.match(/^(name|description):\s*(.*)$/);
    if (kv) out[kv[1]] = kv[2].trim();
  }
  return out;
}

/** Skills from a directory of <name>/SKILL.md (fallback when Pi's own list is not available). */
export function scanSkills(root: string | undefined): SkillRef[] {
  if (!root || !existsSync(root)) return [];
  const out: SkillRef[] = [];
  for (const d of readdirSync(root).sort()) {
    const f = join(root, d, "SKILL.md");
    if (!existsSync(f)) continue;
    const fm = parseFrontmatter(readFileSync(f, "utf8"));
    if (fm.name && fm.description) out.push({ name: fm.name, description: fm.description, filePath: f });
  }
  return out;
}

export class SkillIndex {
  skills = new Map<string, SkillRef>();

  set(list: SkillRef[]) {
    for (const s of list || []) {
      if (s?.name && s?.filePath && !(s as any).disableModelInvocation)
        this.skills.set(String(s.name), { name: String(s.name), description: String(s.description || ""), filePath: String(s.filePath) });
    }
  }

  /** The prompt section: names and one-line descriptions only. */
  section(): string {
    if (!this.skills.size) return "";
    const lines = [...this.skills.values()].map((s) => `- ${s.name}: ${s.description}`);
    return "Skills (instructions for one kind of work, loaded on demand with load_skill {name}; load one when the task matches its description, " +
      "and only then):\n" + lines.join("\n");
  }

  load(name: string): string | null {
    const s = this.skills.get(String(name || "").trim());
    if (!s) return null;
    try {
      return stripFrontmatter(readFileSync(s.filePath, "utf8"));
    } catch {
      return null;
    }
  }
}
