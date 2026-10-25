// B1 regression check with the bundled Pi itself: the models.json the shell writes must make Pi read the main
// model's key from GUI_PI_MAIN_KEY (Pi treats a bare name as a literal key -> 401).
// usage: node scripts/check-pi-key.mjs <models.json written by gpd-core's test> <dir containing node_modules/@earendil-works/pi-coding-agent>
import { readFileSync } from "node:fs";
import { join } from "node:path";
import { pathToFileURL } from "node:url";
const [models, piDir] = process.argv.slice(2);
const mod = join(piDir, "node_modules/@earendil-works/pi-coding-agent/dist/core/resolve-config-value.js");
const { resolveConfigValue } = await import(pathToFileURL(mod).href);
const m = JSON.parse(readFileSync(models, "utf8"));
const providers = Object.values(m.providers || {});
if (!providers.length) throw new Error("no provider in models.json");
const sentinel = "sk-ci-sentinel-" + Math.random().toString(36).slice(2);
for (const p of providers) {
  const got = resolveConfigValue(p.apiKey, { GUI_PI_MAIN_KEY: sentinel });
  if (got !== sentinel) throw new Error(`Pi resolves apiKey ${JSON.stringify(p.apiKey)} to ${JSON.stringify(got)}, not GUI_PI_MAIN_KEY`);
}
// and the 0.3.0 form really was wrong (a literal), so this check can fail
if (resolveConfigValue("GUI_PI_MAIN_KEY", { GUI_PI_MAIN_KEY: sentinel }) === sentinel) throw new Error("check is vacuous");
console.log(`ok: Pi resolves ${providers.map((p) => p.apiKey).join(", ")} from GUI_PI_MAIN_KEY`);
