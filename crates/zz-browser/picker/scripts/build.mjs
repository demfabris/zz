import { readFile } from "node:fs/promises";
import { fileURLToPath } from "node:url";
import { resolve } from "node:path";
import { build } from "esbuild";

const check = process.argv.includes("--check");
const root = fileURLToPath(new URL("..", import.meta.url));
const outfile = resolve(root, "../assets/element-picker.js");
const result = await build({
  absWorkingDir: root,
  entryPoints: ["src/index.ts"],
  bundle: true,
  format: "iife",
  platform: "browser",
  target: "chrome150",
  define: {
    "process.env.NODE_ENV": '"production"',
  },
  legalComments: "eof",
  minify: true,
  outfile,
  write: !check,
});

if (check) {
  const generated = result.outputFiles?.find((file) => file.path === outfile)?.contents;
  const committed = await readFile(outfile);
  if (!generated || !committed.equals(generated)) {
    throw new Error("element-picker.js is stale; run pnpm run build");
  }
}
