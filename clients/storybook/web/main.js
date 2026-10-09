import init, * as zz from "./wasm/zz_storybook.js";

const KNOBS = ["theme", "preset", "radius", "smoothing", "shadow", "contrast", "zoom", "pane-opacity", "pane-glow", "motion"];
const DEFAULTS = {
    theme: "system",
    preset: "default",
    radius: "6",
    smoothing: "4",
    shadow: "1",
    contrast: "1",
    zoom: "1",
    "pane-opacity": "0.5",
    "pane-glow": "1",
    motion: "1",
};

const nav = document.getElementById("nav");
const storyRoot = document.getElementById("story");
const form = document.getElementById("knob-form");

let stories = [];
let current = null;

function parseHash() {
    const [path, query = ""] = location.hash.replace(/^#\/?/, "").split("?");
    const [story, section] = path.split("/");
    return { story, section, params: new URLSearchParams(query) };
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

function knobQuery() {
    const params = new URLSearchParams();
    for (const name of KNOBS) {
        const value = knobValue(name);
        if (value !== DEFAULTS[name]) params.set(name, value);
    }
    return params.toString();
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
    for (const name of KNOBS) {
        const output = form.elements[`${name}-value`];
        if (output) output.value = knobValue(name);
    }
    zz.set_knobs(knobQuery());
    paintChrome();
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
    for (const name of KNOBS) {
        setKnobValue(name, params.get(name) ?? DEFAULTS[name]);
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
renderNav();
renderPresets();
const initial = parseHash();
loadKnobs(initial.params);
applyKnobs();
show(initial.story, initial.section);

form.addEventListener("input", applyKnobs);
form.addEventListener("reset", () => setTimeout(applyKnobs));
window.addEventListener("hashchange", () => {
    const { story, section, params } = parseHash();
    loadKnobs(params);
    applyKnobs();
    show(story, section);
});
matchMedia("(prefers-color-scheme: dark)").addEventListener("change", applyKnobs);

window.storybook = {
    stories: () => stories,
    current: () => current,
    show: (story, section) => {
        show(story, section);
        return globalThis.zpui?.idle();
    },
    setKnobs: (knobs) => {
        for (const [name, value] of Object.entries(knobs)) {
            if (form.elements[name]) setKnobValue(name, String(value));
        }
        applyKnobs();
        return globalThis.zpui?.idle();
    },
    knobs: () => Object.fromEntries(KNOBS.map((name) => [name, knobValue(name)])),
    presets: () => JSON.parse(zz.presets()),
};
