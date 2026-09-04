async function start() {
  const loading = document.querySelector("[data-loading]");

  try {
    const wasm = await import("./wasm/zz_ui_showcase.js");
    await wasm.default();
    const query = new URLSearchParams(location.search);
    let saved = {};
    try { saved = JSON.parse(localStorage.getItem("zz-preview") || "{}"); } catch {}
    const options = { scene: "workspace", width: 1200, height: 760, dark: true, zoom: 1, sidebar: true, gaps: false, blur: false,
      radius: 6, macos: navigator.platform.startsWith("Mac"), ...saved };
    for (const key of ["scene", "settings_section"]) if (query.has(key)) options[key] = query.get(key);
    for (const key of ["width", "height", "zoom", "radius", "pane_margin", "pane_radius", "pane_border", "inactive_opacity"]) if (query.has(key)) options[key] = Number(query.get(key));
    for (const key of ["dark", "sidebar", "gaps", "macos", "blur"]) if (query.has(key)) options[key] = query.get(key) === "true";
    if (query.has("chrome_colors")) {
      try { options.chrome_colors = JSON.parse(query.get("chrome_colors")); } catch {}
    }
    const names = ["regular", "medium", "semibold", "bold", "regular-italic", "medium-italic", "semibold-italic", "bold-italic", "mono"];
    const fonts = await Promise.all(names.map(async (name) => {
      try {
        const response = await fetch(`/__preview-font/${name}`);
        if (!response.ok || !response.headers.get("content-type")?.includes("octet-stream")) return null;
        return new Uint8Array(await response.arrayBuffer());
      } catch { return null; }
    }));
    const systemFont = fonts.slice(0, 8).every(Boolean);
    if (systemFont) fonts.slice(0, 8).forEach((font) => wasm.register_font(font));
    if (fonts[8]) wasm.register_font(fonts[8]);
    options.ui_font = systemFont ? "zz Preview System" : "Inter Variable";
    options.mono_font = fonts[8] ? "Menlo" : "Lilex";
    localStorage.setItem("zz-preview", JSON.stringify(options));
    await wasm.run(JSON.stringify(options));
    loading?.remove();
    parent.postMessage({ type: "zz-preview-ready", fonts: systemFont ? "Local SF · text optical size" : "Fallback: Inter", options }, location.origin);
  } catch (error) {
    console.error("Failed to start the zz UI showcase", error);
    if (loading) {
      loading.dataset.failed = "true";
      loading.textContent = `Could not start the showcase: ${error instanceof Error ? error.message : String(error)}`;
    }
  }
}

start();
