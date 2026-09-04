const fields = ["scene", "width", "height", "zoom", "dark", "sidebar", "gaps"];
const frame = document.querySelector("#preview");
const status = document.querySelector("#status");
const stage = document.querySelector("#stage");
let readyStatus = "Loading…";
let saved = {};
try { saved = JSON.parse(localStorage.getItem("zz-preview") || "{}"); } catch {}
const query = new URLSearchParams(location.search);
for (const key of fields) {
  const field = document.getElementById(key);
  const value = query.get(key) ?? saved[key];
  if (value === undefined || value === null) continue;
  if (field.type === "checkbox") field.checked = String(value) === "true";
  else {
    if (key === "zoom" && ![...field.options].some(option => option.value === String(value))) field.add(new Option(`${Math.round(Number(value) * 100)}%`, String(value)));
    field.value = String(value);
  }
}
function parameters() {
  const params = new URLSearchParams();
  for (const key of ["pin_x", "pin_y"]) if (query.has(key)) params.set(key, query.get(key));
  let saved = {};
  try { saved = JSON.parse(localStorage.getItem("zz-preview") || "{}"); } catch {}
  for (const key of ["radius", "pane_margin", "pane_radius", "pane_border", "inactive_opacity", "settings_section", "chrome_colors", "blur"]) {
    const value = query.get(key) ?? saved[key];
    if (value !== undefined && value !== null) params.set(key, typeof value === "object" ? JSON.stringify(value) : value);
  }
  for (const key of fields) {
    const field = document.getElementById(key);
    params.set(key, field.type === "checkbox" ? field.checked : field.value);
  }
  return params;
}
function refresh() {
  const params = parameters();
  const width = Math.min(3840, Math.max(360, Number(params.get("width")) || 1200));
  const height = Math.min(2160, Math.max(300, Number(params.get("height")) || 760));
  document.getElementById("width").value = width;
  document.getElementById("height").value = height;
  frame.style.width = `${width}px`;
  frame.style.height = `${height}px`;
  params.set("width", width);
  params.set("height", height);
  history.replaceState(null, "", `?${params}`);
  stage.dataset.blur = String(params.get("blur") === "true");
  status.textContent = "Loading…";
  frame.src = `./canvas.html?${params}`;
}
for (const key of fields) document.getElementById(key).addEventListener("change", refresh);
document.getElementById("reset").addEventListener("click", () => {
  localStorage.removeItem("zz-preview");
  location.href = location.pathname;
});
document.getElementById("copy").addEventListener("click", async () => {
  try {
    await navigator.clipboard.writeText(location.href);
    status.textContent = "Link copied";
  } catch { status.textContent = "Copy the URL from the address bar"; }
});
window.addEventListener("message", (event) => {
  if (event.origin !== location.origin || event.source !== frame.contentWindow || event.data?.type !== "zz-preview-ready") return;
  readyStatus = `${event.data.fonts} · ${devicePixelRatio}× display`;
  status.textContent = readyStatus;
  stage.dataset.blur = String(event.data.options.blur);
});
window.addEventListener("storage", (event) => {
  if (event.key !== "zz-preview" || !event.newValue) return;
  let options;
  try { options = JSON.parse(event.newValue); } catch { return; }
  stage.dataset.blur = String(options.blur);
  for (const key of fields) {
    if (!(key in options)) continue;
    const field = document.getElementById(key);
    if (field.type === "checkbox") field.checked = options[key];
    else {
      if (key === "zoom" && ![...field.options].some(option => Number(option.value) === options[key])) field.add(new Option(`${Math.round(options[key] * 100)}%`, String(options[key])));
      field.value = options[key];
    }
  }
  for (const key of Object.keys(options)) query.delete(key);
  const params = parameters();
  history.replaceState(null, "", `?${params}`);
  frame.contentWindow.history.replaceState(null, "", `canvas.html?${params}`);
});
refresh();

const reference = document.getElementById("reference-image");
const opacity = document.getElementById("opacity");
const file = document.getElementById("reference-file");
let referenceUrl;
let backgroundRevision = 0;
const backgroundDatabase = new Promise((resolve, reject) => {
  const request = indexedDB.open("zz-preview-background", 1);
  request.onupgradeneeded = () => request.result.createObjectStore("images");
  request.onsuccess = () => resolve(request.result);
  request.onerror = () => reject(request.error);
});
async function storedBackground(value) {
  const database = await backgroundDatabase;
  return new Promise((resolve, reject) => {
    const transaction = database.transaction("images", value === undefined ? "readonly" : "readwrite");
    const images = transaction.objectStore("images");
    const request = value === undefined ? images.get("background") : value === null ? images.delete("background") : images.put(value, "background");
    transaction.oncomplete = () => resolve(request.result);
    transaction.onerror = () => reject(transaction.error);
    transaction.onabort = () => reject(transaction.error);
  });
}
function showBackground(image) {
  if (referenceUrl) URL.revokeObjectURL(referenceUrl);
  referenceUrl = URL.createObjectURL(image);
  reference.src = referenceUrl;
  reference.hidden = false;
  reference.style.opacity = opacity.value / 100;
  opacity.disabled = false;
  document.getElementById("reference-clear").disabled = false;
}
try { opacity.value = localStorage.getItem("zz-preview-background-opacity") ?? "100"; } catch {}
storedBackground().then((image) => {
  if (image && backgroundRevision === 0) showBackground(image);
}).catch(() => {});
document.getElementById("reference-load").addEventListener("click", () => file.click());
file.addEventListener("change", () => {
  if (!file.files[0]) return;
  backgroundRevision++;
  showBackground(file.files[0]);
  storedBackground(file.files[0]).catch(() => { status.textContent = "Background loaded; browser storage unavailable"; });
});
reference.addEventListener("load", () => {
  status.textContent = `Background: ${reference.naturalWidth}×${reference.naturalHeight} pixels`;
});
reference.addEventListener("error", () => { status.textContent = "Could not load that background image"; });
opacity.addEventListener("input", () => {
  reference.style.opacity = opacity.value / 100;
  try { localStorage.setItem("zz-preview-background-opacity", opacity.value); } catch {}
});
document.getElementById("reference-clear").addEventListener("click", () => {
  backgroundRevision++;
  storedBackground(null).catch(() => { status.textContent = "Could not clear the saved background"; });
  reference.hidden = true;
  reference.removeAttribute("src");
  if (referenceUrl) URL.revokeObjectURL(referenceUrl);
  referenceUrl = undefined;
  file.value = "";
  opacity.disabled = true;
  document.getElementById("reference-clear").disabled = true;
  status.textContent = readyStatus;
});
const pointButton = document.getElementById("point");
const surface = document.getElementById("point-surface");
const pin = document.getElementById("pin");
function showPin() {
  const x = Number(query.get("pin_x"));
  const y = Number(query.get("pin_y"));
  pin.hidden = !query.has("pin_x") || !query.has("pin_y") || !Number.isFinite(x) || !Number.isFinite(y);
  if (!pin.hidden) { pin.style.left = `${x}px`; pin.style.top = `${y}px`; }
}
pointButton.addEventListener("click", () => {
  surface.hidden = !surface.hidden;
  pointButton.setAttribute("aria-pressed", String(!surface.hidden));
});
surface.addEventListener("click", (event) => {
  const bounds = surface.getBoundingClientRect();
  const x = Math.round(event.clientX - bounds.left);
  const y = Math.round(event.clientY - bounds.top);
  query.set("pin_x", x);
  query.set("pin_y", y);
  showPin();
  surface.hidden = true;
  pointButton.setAttribute("aria-pressed", "false");
  history.replaceState(null, "", `?${parameters()}`);
  status.textContent = `Point: ${x}, ${y} · included in Copy link`;
});
showPin();
