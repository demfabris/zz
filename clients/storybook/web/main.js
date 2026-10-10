import init, * as zz from "./wasm/zz_storybook.js";

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
    "row-inset",
    "selection",
    "outline",
    "outline-width",
    "shadow",
    "elevation",
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
    glass: "style",
};

const nav = document.getElementById("nav");
const storyRoot = document.getElementById("story");
const form = document.getElementById("knob-form");

let stories = [];
let glassPresets = {};
let styles = {};
let current = null;

function parseHash() {
    const [path, query = ""] = location.hash.replace(/^#\/?/, "").split("?");
    const [story, section] = path.split("/");
    return { story, section, query, params: new URLSearchParams(query) };
}

function knobValue(name) {
    const field = form.elements[name];
    return field.type === "checkbox" ? (field.checked ? "1" : "0") : field.value;
}

function setKnobValue(name, value) {
    const field = form.elements[name];
    if (field.type === "checkbox") field.checked = value !== "0" && value !== "false";
    else field.value = value;
}

function glassMaterial(glass = knobValue("glass"), style = knobValue("style")) {
    return (glass === "style" ? styles[style]?.glass : glassPresets[glass]) ?? null;
}

function knobDefault(name, glass = knobValue("glass"), style = knobValue("style")) {
    if (LOOK_KNOBS.includes(name)) return String(styles[style]?.[name]);
    if (!GLASS_KNOBS.includes(name)) return DEFAULTS[name];
    const value = glassMaterial(glass, style)?.[name];
    return value === undefined ? undefined : String(value);
}

function isDefault(name) {
    const value = knobValue(name);
    const fallback = knobDefault(name);
    const numeric = form.elements[name].type === "range";
    if (!GLASS_KNOBS.includes(name) && !numeric) return value === fallback;
    if (GLASS_KNOBS.includes(name) && glassMaterial() === null) return true;
    const step = Number(form.elements[name].step) || 0.01;
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

function showLook() {
    const output = document.getElementById("look-json");
    if (!output.hidden) output.value = zz.look();
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
            for (const section of story.sections) {
                sections.append(
                    element("li", {}, element("a", { href: `#/${story.id}/${section.id}`, text: section.name })),
                );
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
    if (current !== story.id) {
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
        zz.show(story.id);
        current = story.id;
        markCurrent(story.id);
        document.title = `${story.name} · zz storybook`;
    }
    if (sectionId) {
        document.getElementById(`${story.id}-${sectionId}`)?.scrollIntoView({ block: "start" });
    } else {
        storyRoot.scrollTo?.(0, 0);
    }
    writeHash(story.id, sectionId);
}

function applyKnobs() {
    form.querySelector("#glass-knobs").hidden = glassMaterial() === null;
    for (const name of KNOBS) {
        const output = form.elements[`${name}-value`];
        if (output) output.value = knobValue(name);
    }
    zz.set_knobs(knobQuery());
    paintChrome();
    showLook();
    const { story, section } = parseHash();
    writeHash(story || current, section);
}

function paintChrome() {
    const theme = JSON.parse(zz.theme());
    const root = document.documentElement.style;
    for (const key of ["background", "raised", "foreground", "muted", "border", "accent"]) {
        root.setProperty(`--zz-${key}`, theme[key]);
    }
    root.setProperty("--zz-radius", `${theme.radius}px`);
    document.documentElement.dataset.mode = theme.mode;
}

function loadKnobs(params) {
    const glass = params.get("glass") ?? DEFAULTS.glass;
    const style = params.get("style") ?? DEFAULTS.style;
    for (const name of KNOBS) {
        const value = params.get(name) ?? knobDefault(name, glass, style);
        if (value !== undefined) setKnobValue(name, value);
    }
}

function renderPresets() {
    const select = form.elements.preset;
    const presets = JSON.parse(zz.presets());
    for (const [label, dark] of [["Dark", true], ["Light", false]]) {
        const group = element("optgroup", { label });
        for (const preset of presets.filter((preset) => preset.dark === dark)) {
            group.append(element("option", { value: preset.id, text: preset.name }));
        }
        select.append(group);
    }
}

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
loadKnobs(initial.params);
applyKnobs();
show(initial.story, initial.section);

form.addEventListener("input", (event) => {
    if (event.target.name === "style") resetStyleKnobs();
    else if (event.target.name === "glass") resetGlassSliders();
    applyKnobs();
});
document.getElementById("export-look").addEventListener("click", () => {
    const output = document.getElementById("look-json");
    output.hidden = false;
    output.value = zz.look();
    output.select();
    navigator.clipboard?.writeText(output.value).catch(() => {});
});
form.addEventListener("reset", () => setTimeout(() => {
    resetStyleKnobs();
    applyKnobs();
}));
window.addEventListener("hashchange", () => {
    const { story, section, query, params } = parseHash();
    if (query) {
        loadKnobs(params);
        applyKnobs();
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
        if ("glass" in knobs) {
            setKnobValue("glass", String(knobs.glass));
            resetGlassSliders();
        }
        for (const [name, value] of Object.entries(knobs)) {
            if (form.elements[name]) setKnobValue(name, String(value));
        }
        applyKnobs();
        return globalThis.zzGpui?.idle();
    },
    knobs: () => Object.fromEntries(KNOBS.map((name) => [name, knobValue(name)])),
    presets: () => JSON.parse(zz.presets()),
    glassPresets: () => glassPresets,
};
