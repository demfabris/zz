import { fallbackContext, fallbackPreview, reactGrabContext } from "./format";

function equal(actual: string, expected: string, label: string): void {
  if (actual !== expected) {
    throw new Error(`${label}\nexpected: ${expected}\nactual:   ${actual}`);
  }
}

function element(attributes: Array<{ name: string; value: string }>, children = 1): Element {
  return {
    id: attributes.find(({ name }) => name === "id")?.value ?? "",
    localName: "div",
    tagName: "DIV",
    attributes,
    classList: [],
    childElementCount: children,
    textContent: children === 0 ? "Save <draft>" : "",
    parentElement: null,
  } as unknown as Element;
}

const card = element([
  { name: "data-slot", value: "card" },
  { name: "class", value: "flex flex-col grow overflow-hidden" },
  { name: "style", value: "color: red" },
  { name: "onclick", value: "doSomething()" },
]);

equal(
  fallbackPreview(card),
  '<div data-slot="card" class="flex flex-col g..." />',
  "keeps the DOM preview React Grab-sized",
);

equal(
  reactGrabContext(card, {
    stackString: "\n  in Card (at /src/card.tsx)\n  in PageContent (at /src/page-shell.tsx)",
    filePath: "/src/card.tsx",
    lineNumber: 12,
    columnNumber: 4,
    componentName: "Card",
    selector: ".card",
  } as Parameters<typeof reactGrabContext>[1]),
  '[<div data-slot="card" class="flex flex-col g..." /> in Card (at /src/card.tsx) in PageContent (at /src/page-shell.tsx)]',
  "compacts the React component stack to one line",
);

Object.defineProperty(globalThis, "CSS", {
  configurable: true,
  value: { escape: (value: string) => value },
});
equal(
  fallbackContext(element([{ name: "id", value: "save" }], 0)),
  '[<div id="save">Save &lt;draft&gt;</div> selector: #save]',
  "provides a safe generic DOM fallback",
);
