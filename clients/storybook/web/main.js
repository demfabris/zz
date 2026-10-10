import init, * as zz from "./wasm/zz_storybook.js";

const CREATOR = "style-creator";
const GLASS_KNOBS = [
    "glass-blur",
    "glass-tint",
    "glass-refraction",
    "glass-bezel",
    "glass-dispersion",
    "glass-saturation",
    "glass-brightness",
    "glass-contrast",
    "glass-specular",
    "glass-glint-width",
    "glass-light",
    "glass-fresnel",
    "glass-edge",
    "glass-edge-width",
    "glass-noise",
];
const LOOK_KNOBS = [
    "radius",
    "smoothing",
    "outline",
    "outline-width",
    "divider",
    "shadow",
    "elevation",
    "density",
    "row-inset",
    "control-fill",
    "selection",
    "font",
    "animation-speed",
];
const SHUFFLED = [
    "radius",
    "smoothing",
    "outline",
    "outline-width",
    "divider",
    "shadow",
    "elevation",
    "density",
    "row-inset",
    "control-fill",
];
const KNOBS = [
    "theme",
    "preset",
    "contrast",
    "zoom",
    "pane-opacity",
    "pane-glow",
    "motion",
    "backdrop",
    "style",
    ...LOOK_KNOBS,
    "icons",
    "glass",
    ...GLASS_KNOBS,
];
const DEFAULTS = {
    theme: "system",
    preset: "default",
    style: "modern",
    contrast: "1",
    zoom: "1",
    "pane-opacity": "0.5",
    "pane-glow": "1",
    motion: "1",
    backdrop: "plain",
    icons: "mac",
    glass: "style",
};

const nav = document.getElementById("nav");
const storyRoot = document.getElementById("story");
const form = document.getElementById("knob-form");
const importDialog = document.getElementById("import-dialog");

let stories = [];
let glassPresets = {};
let styles = {};
let current = null;
const timeline = { past: [], future: [], last: null };

function parseHash() {
    const [path, query = ""] = location.hash.replace(/^#\/?/, "").split("?");
    const [story, section] = path.split("/");
    return { story, section, query, params: new URLSearchParams(query) };
}

function field(name) {
    return form.elements[name];
}

function knobValue(name) {
    const input = field(name);
    return input.type === "checkbox" ? (input.checked ? "1" : "0") : input.value;
}

function setKnobValue(name, value) {
    const input = field(name);
    if (input.type === "checkbox") input.checked = value !== "0" && value !== "false";
    else input.value = value;
}

function isRange(name) {
    return field(name).type === "range";
}

function glassMaterial(glass = knobValue("glass"), style = knobValue("style")) {
    return (glass === "style" ? styles[style]?.glass : glassPresets[glass]) ?? null;
}

function knobDefault(name, glass = knobValue("glass"), style = knobValue("style")) {
    if (LOOK_KNOBS.includes(name)) return String(styles[style]?.[name] ?? "");
    if (!GLASS_KNOBS.includes(name)) return DEFAULTS[name];
    const value = glassMaterial(glass, style)?.[name];
    return value === undefined ? undefined : String(value);
}

function isDefault(name) {
    const value = knobValue(name);
    const fallback = knobDefault(name);
    if (GLASS_KNOBS.includes(name) && glassMaterial() === null) return true;
    if (!isRange(name)) return value === fallback;
    const step = Number(field(name).step) || 0.01;
    return Math.abs(Number(value) - Number(fallback)) < step / 2;
}

function knobQuery() {
    const params = new URLSearchParams();
    for (const name of KNOBS) {
        if (!isDefault(name)) params.set(name, knobValue(name));
    }
    return params.toString();
}

function resetGlassSliders() {
    for (const name of GLASS_KNOBS) {
        const value = knobDefault(name);
        if (value !== undefined) setKnobValue(name, value);
    }
}

function resetStyleKnobs() {
    for (const name of LOOK_KNOBS) setKnobValue(name, knobDefault(name));
    setKnobValue("glass", "style");
    resetGlassSliders();
}

function writeHash(story, section) {
    const query = knobQuery();
    const path = section ? `${story}/${section}` : story;
    const hash = `#/${path}${query ? `?${query}` : ""}`;
    if (location.hash !== hash) history.replaceState(null, "", hash);
}

function element(tag, attributes = {}, ...children) {
    const node = document.createElement(tag);
    for (const [key, value] of Object.entries(attributes)) {
        if (key === "text") node.textContent = value;
        else node.setAttribute(key, value);
    }
    node.append(...children);
    return node;
}

function renderNav() {
    nav.replaceChildren(element("h1", { text: "zz storybook" }));
    const groups = new Map();
    for (const story of stories) {
        if (!groups.has(story.group)) groups.set(story.group, []);
        groups.get(story.group).push(story);
    }
    for (const [group, items] of groups) {
        const list = element("ul");
        for (const story of items) {
            const sections = element("ul", { class: "sections" });
            if (story.id !== CREATOR) {
                for (const section of story.sections) {
                    sections.append(
                        element("li", {}, element("a", { href: `#/${story.id}/${section.id}`, text: section.name })),
                    );
                }
            }
            list.append(
                element(
                    "li",
                    { "data-story": story.id },
                    element("a", { href: `#/${story.id}`, text: story.name }),
                    sections,
                ),
            );
        }
        nav.append(element("h2", { text: group }), list);
    }
}

function markCurrent(storyId) {
    for (const item of nav.querySelectorAll("li[data-story]")) {
        const active = item.dataset.story === storyId;
        item.classList.toggle("current", active);
        item.querySelector("a").toggleAttribute("aria-current", active);
    }
}

function show(storyId, sectionId) {
    const story = stories.find((candidate) => candidate.id === storyId) ?? stories[0];
    if (!story) return;
    const creator = story.id === CREATOR;
    if (current !== story.id) {
        document.body.classList.toggle("creator", creator);
        if (creator) {
            const [canvas] = story.sections;
            storyRoot.replaceChildren(
                element("h1", { class: "visually-hidden", text: story.name }),
                element("div", { id: canvas.mount, class: "mount canvas", "data-section": canvas.id }),
            );
        } else {
            const sections = story.sections.map((section) =>
                element(
                    "section",
                    { id: `${story.id}-${section.id}`, "aria-labelledby": `${story.id}-${section.id}-title` },
                    element("h2", { id: `${story.id}-${section.id}-title`, text: section.name }),
                    element("p", { text: section.summary }),
                    element("div", { id: section.mount, class: "mount", "data-section": section.id }),
                ),
            );
            storyRoot.replaceChildren(
                element("header", {}, element("h1", { text: story.name }), element("p", { text: story.summary })),
                ...sections,
            );
        }
        zz.show(story.id);
        current = story.id;
        markCurrent(story.id);
        document.title = creator ? "Style creator · zz" : `${story.name} · zz storybook`;
    }
    if (sectionId && !creator) {
        document.getElementById(`${story.id}-${sectionId}`)?.scrollIntoView({ block: "start" });
    } else {
        storyRoot.scrollTo?.(0, 0);
    }
    writeHash(story.id, creator ? undefined : sectionId);
}

function paintSliders() {
    for (const input of form.querySelectorAll("input[type=range]")) {
        const min = Number(input.min);
        const max = Number(input.max);
        const fill = ((Number(input.value) - min) / (max - min)) * 100;
        input.parentElement.style.setProperty("--fill", `${Math.max(0, Math.min(100, fill))}%`);
    }
}

function applyKnobs() {
    form.querySelector("#glass-knobs").hidden = glassMaterial() === null;
    for (const name of KNOBS) {
        const output = form.elements[`${name}-value`];
        if (output) output.value = knobValue(name);
    }
    paintSliders();
    zz.set_knobs(knobQuery());
    paintChrome();
    const { story, section } = parseHash();
    writeHash(story || current, section);
}

function remember() {
    const query = knobQuery();
    if (query === timeline.last) return;
    if (timeline.last !== null) timeline.past.push(timeline.last);
    timeline.past = timeline.past.slice(-100);
    timeline.future = [];
    timeline.last = query;
    paintHistory();
}

function travel(from, to) {
    if (from.length === 0) return;
    to.push(timeline.last);
    timeline.last = from.pop();
    loadKnobs(new URLSearchParams(timeline.last));
    applyKnobs();
    paintHistory();
}

function paintHistory() {
    document.querySelector("[data-action=undo]").disabled = timeline.past.length === 0;
    document.querySelector("[data-action=redo]").disabled = timeline.future.length === 0;
}

function paintChrome() {
    const theme = JSON.parse(zz.theme());
    const root = document.documentElement.style;
    for (const key of ["background", "raised", "raised2", "raised3", "foreground", "muted", "border", "accent"]) {
        root.setProperty(`--zz-${key}`, theme[key]);
    }
    root.setProperty("--zz-radius", `${Math.min(theme.radius, 14)}px`);
    document.documentElement.dataset.mode = theme.mode;
}

function loadKnobs(params) {
    const glass = params.get("glass") ?? DEFAULTS.glass;
    const style = params.get("style") ?? DEFAULTS.style;
    setKnobValue("style", style);
    for (const name of KNOBS) {
        const value = params.get(name) ?? knobDefault(name, glass, style);
        if (value !== undefined) setKnobValue(name, value);
    }
}

function renderPresets() {
    const select = field("preset");
    const presets = JSON.parse(zz.presets());
    for (const [label, dark] of [["Dark", true], ["Light", false]]) {
        const group = element("optgroup", { label });
        for (const preset of presets.filter((preset) => preset.dark === dark)) {
            group.append(element("option", { value: preset.id, text: preset.name }));
        }
        select.append(group);
    }
}

function addFontOption(name) {
    const select = field("font");
    if (![...select.options].some((option) => option.value === name)) {
        select.append(element("option", { value: name, text: name }));
    }
}

function toast(message) {
    const node = document.getElementById("toast");
    node.textContent = message;
    node.hidden = false;
    clearTimeout(toast.timer);
    toast.timer = setTimeout(() => (node.hidden = true), 1800);
}

function shuffle() {
    const pick = (name) => {
        const input = field(name);
        const min = Number(input.min);
        const max = Number(input.max);
        const step = Number(input.step) || 0.01;
        const value = min + Math.round((Math.random() * (max - min)) / step) * step;
        setKnobValue(name, String(Number(value.toFixed(4))));
    };
    for (const name of SHUFFLED) pick(name);
    setKnobValue("selection", Math.random() < 0.5 ? "accent" : "wash");
    const glasses = ["style", "off", ...Object.keys(glassPresets)];
    setKnobValue("glass", glasses[Math.floor(Math.random() * glasses.length)]);
    resetGlassSliders();
}

function importLook(json) {
    const knobs = JSON.parse(zz.import_look(json, knobValue("style")));
    if ("glass" in knobs) {
        setKnobValue("glass", knobs.glass);
        resetGlassSliders();
    }
    for (const [name, value] of Object.entries(knobs)) {
        if (name === "font" && value) addFontOption(value);
        if (form.elements[name]) setKnobValue(name, String(value));
    }
}

async function copyLook() {
    const json = zz.look();
    try {
        await navigator.clipboard.writeText(json);
        toast("Look copied as JSON");
    } catch {
        document.getElementById("import-json").value = json;
        importDialog.showModal();
    }
}

const actions = {
    undo: () => travel(timeline.past, timeline.future),
    redo: () => travel(timeline.future, timeline.past),
    reset: () => {
        resetStyleKnobs();
        applyKnobs();
        remember();
    },
    shuffle: () => {
        shuffle();
        applyKnobs();
        remember();
    },
    mode: () => {
        const dark = document.documentElement.dataset.mode === "dark";
        setKnobValue("theme", dark ? "light" : "dark");
        setKnobValue("preset", "default");
        applyKnobs();
        remember();
    },
    import: () => {
        document.getElementById("import-error").value = "";
        importDialog.showModal();
    },
    copy: copyLook,
};

function waitFor(predicate) {
    return new Promise((resolve) => {
        const tick = () => (predicate() ? resolve() : setTimeout(tick, 16));
        tick();
    });
}

await init();
zz.run();
await waitFor(zz.is_ready);
stories = JSON.parse(zz.stories());
glassPresets = JSON.parse(zz.glass_presets());
styles = JSON.parse(zz.interface_styles());
renderNav();
renderPresets();
const initial = parseHash();
const font = initial.params.get("font");
if (font) addFontOption(font);
loadKnobs(initial.params);
applyKnobs();
remember();
show(initial.story, initial.section);

form.addEventListener("input", (event) => {
    if (event.target.name === "style") resetStyleKnobs();
    else if (event.target.name === "glass") resetGlassSliders();
    applyKnobs();
});
form.addEventListener("change", remember);
form.addEventListener("reset", () =>
    setTimeout(() => {
        loadKnobs(new URLSearchParams());
        applyKnobs();
        remember();
    }),
);
document.addEventListener("click", (event) => {
    const button = event.target.closest("[data-action]");
    if (button && actions[button.dataset.action]) {
        event.preventDefault();
        actions[button.dataset.action]();
    }
});
document.addEventListener("keydown", (event) => {
    if (!(event.metaKey || event.ctrlKey) || event.key.toLowerCase() !== "z") return;
    if (event.target.closest?.("textarea, input[type=text]")) return;
    event.preventDefault();
    actions[event.shiftKey ? "redo" : "undo"]();
});
importDialog.addEventListener("close", () => {
    if (importDialog.returnValue !== "apply") return;
    try {
        importLook(document.getElementById("import-json").value);
        applyKnobs();
        remember();
        toast("Look imported");
    } catch (error) {
        document.getElementById("import-error").value = String(error);
        importDialog.showModal();
    }
});
document.getElementById("font-file").addEventListener("change", async (event) => {
    const [file] = event.target.files;
    const status = document.getElementById("font-status");
    if (!file) return;
    try {
        const names = JSON.parse(zz.add_font(new Uint8Array(await file.arrayBuffer())));
        if (names.length === 0) throw new Error("no new font family in that file");
        names.forEach(addFontOption);
        setKnobValue("font", names[0]);
        status.value = names[0];
        applyKnobs();
        remember();
    } catch (error) {
        status.value = String(error.message ?? error);
    }
    event.target.value = "";
});
window.addEventListener("hashchange", () => {
    const { story, section, query, params } = parseHash();
    if (query) {
        loadKnobs(params);
        applyKnobs();
        remember();
    }
    show(story, section);
});
matchMedia("(prefers-color-scheme: dark)").addEventListener("change", applyKnobs);

window.storybook = {
    stories: () => stories,
    current: () => current,
    show: (story, section) => {
        show(story, section);
        return globalThis.zzGpui?.idle();
    },
    setKnobs: (knobs) => {
        if ("style" in knobs) {
            setKnobValue("style", String(knobs.style));
            resetStyleKnobs();
        }
        if ("glass" in knobs) {
            setKnobValue("glass", String(knobs.glass));
            resetGlassSliders();
        }
        for (const [name, value] of Object.entries(knobs)) {
            if (form.elements[name]) setKnobValue(name, String(value));
        }
        applyKnobs();
        remember();
        return globalThis.zzGpui?.idle();
    },
    knobs: () => Object.fromEntries(KNOBS.map((name) => [name, knobValue(name)])),
    look: () => JSON.parse(zz.look()),
    importLook: (look) => {
        importLook(typeof look === "string" ? look : JSON.stringify(look));
        applyKnobs();
        remember();
        return globalThis.zzGpui?.idle();
    },
    presets: () => JSON.parse(zz.presets()),
    glassPresets: () => glassPresets,
};
