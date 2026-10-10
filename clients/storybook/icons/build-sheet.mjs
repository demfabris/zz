#!/usr/bin/env node
import { execFileSync } from "node:child_process";
import { existsSync, mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const here = dirname(fileURLToPath(import.meta.url));
const root = join(here, "../../..");
const out = process.argv[2] ?? join(root, "target/icon-sheet/zz-icons.html");

const sets = [JSON.parse(readFileSync(join(root, "crates/zz-gpui-kit/src/icon/mac.json"), "utf8"))];
const set = sets[0];
const renderer = readFileSync(join(here, "glyph.js"), "utf8");
const template = readFileSync(join(here, "sheet.html"), "utf8");
const lilex = readFileSync(join(root, "clients/web/assets/fonts/lilex/Lilex-Regular.ttf")).toString("base64");

const usage = {};
for (const icon of set.icons) {
  let files = 0;
  try {
    files = execFileSync("rg", ["-l", `IconName::${icon.variant}\\b`, "crates", "clients", "-g", "*.rs",
      "-g", "!crates/zz-gpui-kit/src/icon/**", "-g", "!clients/storybook/**"], { cwd: root, encoding: "utf8" })
      .split("\n").filter(Boolean).length;
  } catch {
    files = 0;
  }
  usage[icon.variant] = { files, site: existsSync(join(root, "site/src/icons", `${icon.name}.svg`)) };
}

const page = template
  .replace("/*LILEX*/", lilex)
  .replace("/*RENDERER*/", renderer)
  .replace("/*SETS*/", JSON.stringify(sets))
  .replace("/*USAGE*/", JSON.stringify(usage));
mkdirSync(dirname(out), { recursive: true });
writeFileSync(out, page);
console.log(`${set.icons.length} icons -> ${out} (${(page.length / 1024).toFixed(0)} KB)`);
