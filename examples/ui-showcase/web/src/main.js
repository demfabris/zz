async function start() {
  const loading = document.querySelector("[data-loading]");

  try {
    const wasm = await import("./wasm/zz_ui_showcase.js");
    await wasm.default();
    await wasm.run();
    loading?.remove();
  } catch (error) {
    console.error("Failed to start the zz UI showcase", error);
    if (loading) {
      loading.dataset.failed = "true";
      loading.innerHTML = `
        <strong>Could not start the showcase</strong>
        <span>${error instanceof Error ? error.message : String(error)}</span>
        <small>Open the browser console for the full error.</small>
      `;
    }
  }
}

start();
