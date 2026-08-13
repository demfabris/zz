import type { ReactGrabElementContext } from "react-grab/primitives";

const MAX_FALLBACK_ATTRIBUTE_LENGTH = 64;
const MAX_FALLBACK_CLASS_LENGTH = 15;
const MAX_FALLBACK_TEXT_LENGTH = 80;

function truncate(value: string, maximum: number): string {
  if (value.length <= maximum) return value;
  return `${value.slice(0, maximum)}...`;
}

function escapeAttribute(value: string): string {
  return value.replaceAll("&", "&amp;").replaceAll('"', "&quot;");
}

function escapeText(value: string): string {
  return value.replaceAll("&", "&amp;").replaceAll("<", "&lt;").replaceAll(">", "&gt;");
}

function compactWhitespace(value: string): string {
  return value.replace(/\s+/g, " ").trim();
}

function fallbackSelector(element: Element): string {
  if (element.id) return `#${CSS.escape(element.id)}`;

  const parts: string[] = [];
  let current: Element | null = element;
  while (current && current !== document.documentElement && parts.length < 5) {
    let part = current.localName;
    const stableClass = Array.from(current.classList).find(
      (name) => name.length > 0 && name.length <= 48 && !/[\[\](){}]/.test(name),
    );
    if (stableClass) {
      part += `.${CSS.escape(stableClass)}`;
    } else if (current.parentElement) {
      const siblings = Array.from(current.parentElement.children).filter(
        (sibling) => sibling.localName === current?.localName,
      );
      if (siblings.length > 1) part += `:nth-of-type(${siblings.indexOf(current) + 1})`;
    }
    parts.unshift(part);
    current = current.parentElement;
  }
  return parts.join(" > ");
}

export function fallbackPreview(element: Element): string {
  const tagName = element.localName || element.tagName.toLowerCase();
  const attributes = Array.from(element.attributes)
    .filter(({ name }) => name !== "style" && !name.toLowerCase().startsWith("on"))
    .slice(0, 8)
    .map(({ name, value }) => {
      if (!value) return name;
      const maximum = name === "class" ? MAX_FALLBACK_CLASS_LENGTH : MAX_FALLBACK_ATTRIBUTE_LENGTH;
      return `${name}="${escapeAttribute(truncate(compactWhitespace(value), maximum))}"`;
    })
    .join(" ");
  const opening = attributes ? `<${tagName} ${attributes}` : `<${tagName}`;
  const shouldIncludeText =
    element.childElementCount === 0 ||
    ["a", "button", "label", "option", "summary"].includes(tagName);
  const text = shouldIncludeText
    ? escapeText(truncate(compactWhitespace(element.textContent ?? ""), MAX_FALLBACK_TEXT_LENGTH))
    : "";
  return text ? `${opening}>${text}</${tagName}>` : `${opening} />`;
}

export function fallbackContext(element: Element): string {
  const selector = fallbackSelector(element);
  return `[${fallbackPreview(element)}${selector ? ` selector: ${selector}` : ""}]`;
}

export function reactGrabContext(
  element: Element,
  context: ReactGrabElementContext,
): string {
  const stack = compactWhitespace(context.stackString);
  if (stack) return `[${fallbackPreview(element)} ${stack}]`;

  if (context.filePath) {
    const line = context.lineNumber === null ? "" : `:${context.lineNumber}`;
    const column = context.columnNumber === null ? "" : `:${context.columnNumber}`;
    const component = context.componentName ?? element.localName;
    return `[${fallbackPreview(element)} in ${component} (at ${context.filePath}${line}${column})]`;
  }

  return context.selector ? `[${fallbackPreview(element)} selector: ${context.selector}]` : "";
}
