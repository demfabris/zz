import { getElementContext } from "react-grab/primitives";
import { fallbackContext, reactGrabContext } from "./format";

const API_NAME = "__zzElementPicker";
const QUERY_NAME = "__zzElementPickerQuery";
const OVERLAY_ATTRIBUTE = "data-zz-element-picker";
const CONTEXT_TIMEOUT_MS = 9_000;

/// Everything the native side needs to clip a screenshot around the pick.
/// CSS pixels, viewport-relative rect plus the scroll offsets that turn it into
/// page coordinates.
interface PickGeometry {
  x: number;
  y: number;
  width: number;
  height: number;
  scrollX: number;
  scrollY: number;
  viewportWidth: number;
  viewportHeight: number;
}

interface ElementPickerAppearance {
  highlightOutline: string;
  highlightFill: string;
  highlightContrast: string;
  previewBackground: string;
  previewForeground: string;
  previewBorder: string;
  shadow: string | null;
  radius: number;
  fontFamily: string;
  pageZoom: number;
}

type PickMessage =
  | { version: 1; kind: "picked"; token: string; text: string; geometry?: PickGeometry }
  | { version: 1; kind: "cancelled"; token: string }
  | { version: 1; kind: "failed"; token: string };

type QueryFunction = (options: {
  request: string;
  persistent?: boolean;
  onSuccess?: (response: string) => void;
  onFailure?: (code: number, message: string) => void;
}) => void;

interface PickerApi {
  start(token: string, appearance: ElementPickerAppearance): void;
  cancel(): void;
}

declare global {
  interface Window {
    __zzElementPicker?: PickerApi;
    __zzElementPickerQuery?: QueryFunction;
  }
}

let cleanupActivePicker: (() => void) | null = null;

function send(message: PickMessage): void {
  const query = window[QUERY_NAME as keyof Window] as QueryFunction | undefined;
  if (typeof query !== "function") return;
  query({
    request: JSON.stringify(message),
    onSuccess: () => undefined,
    onFailure: () => {
      if (message.kind !== "picked") return;
      query({
        request: JSON.stringify({ version: 1, kind: "failed", token: message.token }),
        onSuccess: () => undefined,
        onFailure: () => undefined,
      });
    },
  });
}

function geometryFor(element: Element): PickGeometry | undefined {
  const rect = element.getBoundingClientRect();
  if (rect.width <= 0 || rect.height <= 0) return undefined;
  return {
    x: rect.left,
    y: rect.top,
    width: rect.width,
    height: rect.height,
    scrollX: window.scrollX,
    scrollY: window.scrollY,
    viewportWidth: window.innerWidth,
    viewportHeight: window.innerHeight,
  };
}

async function contextForElement(element: Element): Promise<string> {
  try {
    const context = await Promise.race([
      getElementContext(element),
      new Promise<never>((_, reject) => {
        window.setTimeout(() => reject(new Error("element context timed out")), CONTEXT_TIMEOUT_MS);
      }),
    ]);
    return reactGrabContext(element, context) || fallbackContext(element);
  } catch {
    return fallbackContext(element);
  }
}

function elementAt(clientX: number, clientY: number, host: HTMLElement): Element | null {
  return (
    document
      .elementsFromPoint(clientX, clientY)
      .find(
        (element) =>
          element !== host &&
          element !== document.documentElement &&
          element !== document.body &&
          !element.hasAttribute(OVERLAY_ATTRIBUTE),
      ) ?? null
  );
}

function labelFor(element: Element): string {
  if (element.id) return `${element.localName}#${element.id}`;
  const className = Array.from(element.classList).find((name) => name.length <= 32);
  return className ? `${element.localName}.${className}` : element.localName;
}

function adaptiveRadius(radius: number, width: number, height: number, scale: number): number {
  const cap = 0.45 * Math.min(width, height);
  return cap > 0 ? cap * Math.tanh((radius * scale) / cap) : 0;
}

function start(token: string, requestedAppearance: ElementPickerAppearance): void {
  cleanupActivePicker?.();
  cleanupActivePicker = null;
  if (!token || !requestedAppearance) return;

  const appearance = requestedAppearance;
  const radius = Number.isFinite(appearance.radius)
    ? Math.min(Math.max(appearance.radius, 0), 1_000)
    : 0;
  const pageZoom = Number.isFinite(appearance.pageZoom)
    ? Math.min(Math.max(appearance.pageZoom, 0.25), 5)
    : 1;
  const scale = 1 / pageZoom;

  const host = document.createElement("div");
  host.setAttribute(OVERLAY_ATTRIBUTE, "");
  host.style.cssText =
    "position:fixed;inset:0;z-index:2147483646;pointer-events:none;contain:strict";
  host.style.setProperty("--zz-picker-highlight-outline", appearance.highlightOutline);
  host.style.setProperty("--zz-picker-highlight-fill", appearance.highlightFill);
  host.style.setProperty("--zz-picker-highlight-contrast", appearance.highlightContrast);
  host.style.setProperty("--zz-picker-preview-background", appearance.previewBackground);
  host.style.setProperty("--zz-picker-preview-foreground", appearance.previewForeground);
  host.style.setProperty("--zz-picker-preview-border", appearance.previewBorder);
  host.style.setProperty("--zz-picker-preview-shadow", appearance.shadow ?? "transparent");
  host.style.setProperty(
    "--zz-picker-font-family",
    `${JSON.stringify(appearance.fontFamily)}, monospace`,
  );
  host.style.setProperty("--zz-picker-outline-width", `${2 * scale}px`);
  host.style.setProperty("--zz-picker-hairline", `${scale}px`);
  host.style.setProperty("--zz-picker-viewport-inset", `${16 * scale}px`);
  host.style.setProperty("--zz-picker-label-max-width", `${360 * scale}px`);
  host.style.setProperty("--zz-picker-padding-y", `${3 * scale}px`);
  host.style.setProperty("--zz-picker-padding-x", `${6 * scale}px`);
  host.style.setProperty("--zz-picker-font-size", `${11 * scale}px`);
  host.style.setProperty("--zz-picker-line-height", `${15 * scale}px`);
  host.style.setProperty("--zz-picker-shadow-y", `${scale}px`);
  host.style.setProperty("--zz-picker-shadow-blur", `${3 * scale}px`);
  const shadow = host.attachShadow({ mode: "closed" });
  const style = document.createElement("style");
  style.textContent = `
    :host { all: initial; }
    .outline {
      position: fixed;
      display: none;
      box-sizing: border-box;
      border: var(--zz-picker-outline-width) solid var(--zz-picker-highlight-outline);
      background: var(--zz-picker-highlight-fill);
      box-shadow: 0 0 0 var(--zz-picker-hairline) var(--zz-picker-highlight-contrast);
      corner-shape: squircle;
      pointer-events: none;
    }
    .label {
      position: fixed;
      display: none;
      box-sizing: border-box;
      max-width: min(
        var(--zz-picker-label-max-width),
        calc(100vw - var(--zz-picker-viewport-inset))
      );
      overflow: hidden;
      padding: var(--zz-picker-padding-y) var(--zz-picker-padding-x);
      border: var(--zz-picker-hairline) solid var(--zz-picker-preview-border);
      background: var(--zz-picker-preview-background);
      box-shadow:
        0 var(--zz-picker-shadow-y) var(--zz-picker-shadow-blur)
        var(--zz-picker-preview-shadow);
      color: var(--zz-picker-preview-foreground);
      corner-shape: squircle;
      font-family: var(--zz-picker-font-family);
      font-size: var(--zz-picker-font-size);
      font-weight: 500;
      line-height: var(--zz-picker-line-height);
      text-overflow: ellipsis;
      white-space: nowrap;
      pointer-events: none;
    }
  `;
  const outline = document.createElement("div");
  outline.className = "outline";
  const label = document.createElement("div");
  label.className = "label";
  shadow.append(style, outline, label);

  const cursorStyle = document.createElement("style");
  cursorStyle.setAttribute(OVERLAY_ATTRIBUTE, "");
  cursorStyle.textContent =
    "html[data-zz-picking], html[data-zz-picking] body, html[data-zz-picking] body * { cursor: crosshair !important; }";
  document.documentElement.setAttribute("data-zz-picking", "");
  document.documentElement.append(cursorStyle, host);

  let selected: Element | null = null;
  let pending = false;

  const hide = (): void => {
    outline.style.display = "none";
    label.style.display = "none";
  };

  const positionLabel = (target: DOMRect): void => {
    label.style.display = "block";
    label.style.left = "0px";
    label.style.top = "0px";
    const measured = label.getBoundingClientRect();
    const edgeGap = 8 * scale;
    const labelGap = 4 * scale;
    const maxLeft = Math.max(edgeGap, window.innerWidth - measured.width - edgeGap);
    const maxTop = Math.max(edgeGap, window.innerHeight - measured.height - edgeGap);
    const above = target.top - measured.height - labelGap;
    const below = target.bottom + labelGap;
    label.style.left = `${Math.min(Math.max(target.left, edgeGap), maxLeft)}px`;
    label.style.top = `${above >= edgeGap ? above : Math.min(Math.max(below, edgeGap), maxTop)}px`;
    label.style.borderRadius = `${adaptiveRadius(
      radius,
      measured.width,
      measured.height,
      scale,
    )}px`;
  };

  const paint = (): void => {
    if (!selected || !selected.isConnected) {
      hide();
      return;
    }
    const rect = selected.getBoundingClientRect();
    if (rect.width <= 0 || rect.height <= 0) {
      hide();
      return;
    }
    outline.style.display = "block";
    outline.style.left = `${rect.left}px`;
    outline.style.top = `${rect.top}px`;
    outline.style.width = `${rect.width}px`;
    outline.style.height = `${rect.height}px`;
    outline.style.borderRadius = `${adaptiveRadius(
      radius,
      rect.width,
      rect.height,
      scale,
    )}px`;
    label.textContent = labelFor(selected);
    positionLabel(rect);
  };

  const cleanup = (): void => {
    window.removeEventListener("pointermove", onPointerMove, true);
    window.removeEventListener("pointerdown", onPointerDown, true);
    window.removeEventListener("pointerup", onPointerUp, true);
    window.removeEventListener("keydown", onKeyDown, true);
    window.removeEventListener("scroll", paint, true);
    window.removeEventListener("resize", paint);
    document.documentElement.removeAttribute("data-zz-picking");
    cursorStyle.remove();
    host.remove();
    if (cleanupActivePicker === cleanup) cleanupActivePicker = null;
  };

  const cancel = (notify: boolean): void => {
    cleanup();
    if (notify) send({ version: 1, kind: "cancelled", token });
  };

  function onPointerMove(event: PointerEvent): void {
    if (pending) return;
    selected = elementAt(event.clientX, event.clientY, host);
    paint();
  }

  function onPointerDown(event: PointerEvent): void {
    if (event.button !== 0) return;
    if (!pending) {
      selected = elementAt(event.clientX, event.clientY, host);
      paint();
    }
    event.preventDefault();
    event.stopImmediatePropagation();
  }

  function onPointerUp(event: PointerEvent): void {
    if (event.button !== 0) return;
    event.preventDefault();
    event.stopImmediatePropagation();
    if (!selected || pending) return;
    pending = true;
    const element = selected;
    // Measured now: resolving the source context is async and the page can
    // scroll or reflow before it settles.
    const geometry = geometryFor(element);
    label.textContent = "Inspecting source…";
    positionLabel(element.getBoundingClientRect());
    void contextForElement(element)
      .then((text) => {
        cleanup();
        send({ version: 1, kind: "picked", token, text, geometry });
      })
      .catch(() => {
        cleanup();
        send({ version: 1, kind: "failed", token });
      });
  }

  function onKeyDown(event: KeyboardEvent): void {
    if (event.key !== "Escape") return;
    event.preventDefault();
    event.stopImmediatePropagation();
    cancel(true);
  }

  window.addEventListener("pointermove", onPointerMove, true);
  window.addEventListener("pointerdown", onPointerDown, true);
  window.addEventListener("pointerup", onPointerUp, true);
  window.addEventListener("keydown", onKeyDown, true);
  window.addEventListener("scroll", paint, true);
  window.addEventListener("resize", paint);
  cleanupActivePicker = cleanup;
}

function cancel(): void {
  cleanupActivePicker?.();
  cleanupActivePicker = null;
}

if (!window[API_NAME as keyof Window]) {
  Object.defineProperty(window, API_NAME, {
    configurable: false,
    enumerable: false,
    writable: false,
    value: Object.freeze({ start, cancel } satisfies PickerApi),
  });
}
