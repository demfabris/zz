import { defineConfig } from "vite";
import { readFile } from "node:fs/promises";
import { fileURLToPath } from "node:url";

const fontCache = new URL("../../../target/ui-showcase-fonts/", import.meta.url);
const faces = ["regular", "medium", "semibold", "bold"];
const fonts = {
  ...Object.fromEntries(faces.flatMap((name) => [name, `${name}-italic`]).map((name) =>
    [name, fileURLToPath(new URL(`${name}.ttf`, fontCache))])),
  mono: "/System/Library/Fonts/Menlo.ttc",
};

export default defineConfig({
  base: "./",
  plugins: [{
    name: "local-preview-fonts",
    configureServer(server) {
      server.middlewares.use(async (request, response, next) => {
        const key = request.url?.split("?")[0]?.replace("/__preview-font/", "");
        if (!request.url?.startsWith("/__preview-font/") || !Object.hasOwn(fonts, key)) return next();
        try {
          const bytes = await readFile(fonts[key]);
          response.setHeader("Content-Type", "application/octet-stream");
          response.end(bytes);
        } catch {
          response.statusCode = 404;
          response.end();
        }
      });
    },
  }],
  build: {
    target: "esnext",
    rolldownOptions: { input: ["index.html", "canvas.html"] },
    sourcemap: true,
  },
  server: {
    host: "127.0.0.1",
    // Single source of truth for the showcase port. `strictPort` makes a
    // clash fail loudly rather than silently drifting to another port, which
    // would leave the printed URL and the docs pointing at the wrong place.
    port: 3131,
    strictPort: true,
    open: true,
    headers: {
      "Cross-Origin-Embedder-Policy": "require-corp",
      "Cross-Origin-Opener-Policy": "same-origin",
    },
  },
  optimizeDeps: {
    exclude: ["./src/wasm"],
  },
});
