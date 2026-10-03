export type ShortcutAction =
  | "toggleRecording"
  | "togglePlayback"
  | "insertMarker"
  | "previousMarker"
  | "nextMarker"
  | "focusPidSearch"
  | "captureScreenshot"
  | "zoomIn"
  | "zoomOut";

export type ShortcutBindings = Record<ShortcutAction, string | null>;

export type ParsedShortcut = {
  key: string;
  ctrl: boolean;
  shift: boolean;
  alt: boolean;
  meta: boolean;
};

export const shortcutActions: Array<{ id: ShortcutAction; label: string }> = [
  { id: "toggleRecording", label: "Start / stop recording" },
  { id: "togglePlayback", label: "Play / pause replay" },
  { id: "insertMarker", label: "Insert marker" },
  { id: "previousMarker", label: "Previous marker" },
  { id: "nextMarker", label: "Next marker" },
  { id: "focusPidSearch", label: "Focus PID search" },
  { id: "captureScreenshot", label: "Capture graph screenshot" },
  { id: "zoomIn", label: "Zoom in" },
  { id: "zoomOut", label: "Zoom out" },
];

export const defaultShortcuts: ShortcutBindings = {
  toggleRecording: "Ctrl+Shift+R",
  togglePlayback: "Ctrl+Shift+P",
  insertMarker: "Space",
  previousMarker: "Ctrl+ArrowLeft",
  nextMarker: "Ctrl+ArrowRight",
  focusPidSearch: "/",
  captureScreenshot: "Ctrl+Shift+S",
  zoomIn: "Ctrl+Shift+Plus",
  zoomOut: "Ctrl+Minus",
};

const namedKeys: Record<string, string> = {
  space: " ",
  plus: "+",
  minus: "-",
  arrowleft: "arrowleft",
  arrowright: "arrowright",
  arrowup: "arrowup",
  arrowdown: "arrowdown",
  enter: "enter",
  escape: "escape",
  tab: "tab",
  backspace: "backspace",
  delete: "delete",
  home: "home",
  end: "end",
  pageup: "pageup",
  pagedown: "pagedown",
  insert: "insert",
  slash: "/",
};

function keyName(value: string): string | null {
  const name = value.trim().toLowerCase();
  if (namedKeys[name]) return namedKeys[name];
  if (/^[a-z0-9]$/i.test(name)) return name.toLowerCase();
  if (/^[!"#$%&'()*,-./:;<=>?@[\\\]^_`{|}~]$/.test(name)) return name;
  if (/^f(?:[1-9]|1[0-2])$/i.test(name)) return name;
  return null;
}

function eventKey(value: string): string | null {
  if (value === " ") return " ";
  if (value.length === 1) return value.toLowerCase();
  return keyName(value);
}

function displayKey(key: string): string {
  if (key === " ") return "Space";
  if (key === "+") return "Plus";
  if (key === "-") return "Minus";
  if (key.startsWith("arrow")) return `Arrow${key.slice(5, 6).toUpperCase()}${key.slice(6)}`;
  if (/^f\d+$/.test(key)) return key.toUpperCase();
  if (["enter", "escape", "tab", "backspace", "delete", "home", "end", "pageup", "pagedown", "insert"].includes(key)) {
    return key[0].toUpperCase() + key.slice(1);
  }
  return key.toUpperCase();
}

function formatShortcut(shortcut: ParsedShortcut): string {
  const modifiers = [
    shortcut.ctrl && "Ctrl",
    shortcut.alt && "Alt",
    shortcut.shift && "Shift",
    shortcut.meta && "Meta",
  ].filter(Boolean);
  return [...modifiers, displayKey(shortcut.key)].join("+");
}

export function parseShortcut(value: string): ParsedShortcut | null {
  const parts = value.split("+").map((part) => part.trim()).filter(Boolean);
  if (!parts.length) return null;

  const modifiers = { ctrl: false, shift: false, alt: false, meta: false };
  let keyPart: string | undefined;
  for (const part of parts) {
    const modifier = part.toLowerCase();
    if (modifier === "ctrl" || modifier === "control") {
      if (modifiers.ctrl) return null;
      modifiers.ctrl = true;
    } else if (modifier === "shift") {
      if (modifiers.shift) return null;
      modifiers.shift = true;
    } else if (modifier === "alt" || modifier === "option") {
      if (modifiers.alt) return null;
      modifiers.alt = true;
    } else if (modifier === "meta" || modifier === "cmd" || modifier === "command") {
      if (modifiers.meta) return null;
      modifiers.meta = true;
    } else if (keyPart !== undefined) {
      return null;
    } else {
      keyPart = part;
    }
  }

  if (keyPart === undefined) return null;
  const key = keyName(keyPart);
  return key === null ? null : { key, ...modifiers };
}

export function matchesShortcut(
  shortcut: ParsedShortcut,
  event: Pick<KeyboardEvent, "key" | "ctrlKey" | "shiftKey" | "altKey" | "metaKey">,
): boolean {
  const key = eventKey(event.key);
  return key !== null
    && shortcut.key === key
    && shortcut.ctrl === event.ctrlKey
    && shortcut.shift === event.shiftKey
    && shortcut.alt === event.altKey
    && shortcut.meta === event.metaKey;
}

export function allowsSpaceMarkerShortcut(key: string, graphSliderFocused: boolean): boolean {
  return key === " " && graphSliderFocused;
}

export function shortcutFromEvent(
  event: Pick<KeyboardEvent, "key" | "ctrlKey" | "shiftKey" | "altKey" | "metaKey">,
): string | null {
  const key = eventKey(event.key);
  if (key === null || ["control", "shift", "alt", "meta"].includes(key)) return null;
  return formatShortcut({
    key,
    ctrl: event.ctrlKey,
    shift: event.shiftKey,
    alt: event.altKey,
    meta: event.metaKey,
  });
}

export function restoreShortcutBindings(serialized: string | null, defaults: ShortcutBindings): ShortcutBindings {
  let saved: Partial<ShortcutBindings> = {};
  try {
    const value: unknown = JSON.parse(serialized ?? "null");
    if (value && typeof value === "object" && !Array.isArray(value)) saved = value as Partial<ShortcutBindings>;
  } catch { /* Ignore malformed optional preferences. */ }

  const restored = { ...defaults };
  const used = new Set<string>();
  for (const { id } of shortcutActions) {
    const candidate = saved[id] === null || (typeof saved[id] === "string" && parseShortcut(saved[id]))
      ? saved[id]
      : defaults[id];
    const candidateShortcut = candidate === null ? null : parseShortcut(candidate!);
    const signature = candidateShortcut ? formatShortcut(candidateShortcut).toLowerCase() : null;
    if (signature && used.has(signature)) {
      const fallback = defaults[id] === null ? null : parseShortcut(defaults[id]!);
      const fallbackSignature = fallback ? formatShortcut(fallback).toLowerCase() : null;
      restored[id] = fallbackSignature && !used.has(fallbackSignature) ? defaults[id] : null;
    } else {
      restored[id] = candidate ?? null;
    }
    if (restored[id]) used.add(formatShortcut(parseShortcut(restored[id]!)!).toLowerCase());
  }
  return restored;
}
