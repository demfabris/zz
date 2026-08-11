import { defineConfig } from "vite";

export default defineConfig({
  base: "./",
  build: {
    target: "esnext",
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
