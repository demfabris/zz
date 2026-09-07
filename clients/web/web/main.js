const loading = document.querySelector("[data-loading]");
const status = document.querySelector("[data-status]");
const detail = document.querySelector("[data-detail]");
const retry = document.querySelector("[data-retry]");
let ready = false;

retry.addEventListener("click", () => location.reload());

function fail(error) {
  console.error("zz stopped", error);
  if (!loading.isConnected) document.body.append(loading);
  loading.dataset.failed = "true";
  status.textContent = ready ? "zz stopped. Reload to reconnect." : "Could not open zz";
  detail.textContent = error instanceof Error ? error.message : String(error);
  detail.hidden = false;
  retry.hidden = false;
}

window.addEventListener("error", (event) => fail(event.error || event.message));
window.addEventListener("unhandledrejection", (event) => fail(event.reason));

async function start() {
  if (location.protocol === "file:") {
    throw new Error("Open zz through the web gateway instead of opening this file directly.");
  }
  if (typeof WebAssembly !== "object") {
    throw new Error("This browser does not support WebAssembly. Open zz in a current browser.");
  }
  const wasm = await import("./wasm/zz_web_client.js");
  await wasm.default();
  wasm.run();
  const started = performance.now();
  await new Promise((resolve, reject) => {
    function check() {
      if (wasm.is_ready()) {
        resolve();
      } else if (performance.now() - started > 30000) {
        const graphicsError = [...document.querySelectorAll("body > p")]
          .find((element) => element.textContent.startsWith("Failed to initialize browser graphics:"));
        reject(new Error(graphicsError?.textContent || "Could not start graphics. Enable hardware acceleration and reload."));
      } else {
        requestAnimationFrame(check);
      }
    }
    check();
  });
  ready = true;
  loading.remove();
}

start().catch(fail);
