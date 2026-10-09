import { build } from "esbuild";
import { fileURLToPath } from "node:url";

const root = fileURLToPath(new URL("..", import.meta.url));
const result = await build({
  absWorkingDir: root,
  entryPoints: ["src/format.test.ts"],
  bundle: true,
  format: "iife",
  platform: "node",
  target: "node18",
  write: false,
});

const output = result.outputFiles?.[0];
if (!output) throw new Error("element picker test bundle was not generated");
new Function(new TextDecoder().decode(output.contents))();
