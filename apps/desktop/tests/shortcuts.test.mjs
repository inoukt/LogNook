import assert from "node:assert/strict";
import test from "node:test";
import {
  defaultShortcuts,
  allowsSpaceMarkerShortcut,
  matchesShortcut,
  parseShortcut,
  restoreShortcutBindings,
  shortcutActions,
  shortcutFromEvent,
} from "../src/shortcuts.ts";

test("shortcut parser normalizes modifiers and special keys", () => {
  assert.deepEqual(parseShortcut(" Ctrl + Shift + R "), {
    key: "r", ctrl: true, shift: true, alt: false, meta: false,
  });
  assert.deepEqual(parseShortcut("Space"), {
    key: " ", ctrl: false, shift: false, alt: false, meta: false,
  });
  assert.deepEqual(parseShortcut("Ctrl+ArrowLeft"), {
    key: "arrowleft", ctrl: true, shift: false, alt: false, meta: false,
  });
  assert.deepEqual(parseShortcut("Ctrl+Plus"), {
    key: "+", ctrl: true, shift: false, alt: false, meta: false,
  });
  assert.equal(parseShortcut("Ctrl+Shift+UnknownKey"), null);
  assert.equal(parseShortcut("Ctrl+Ctrl+R"), null);
});

test("shortcut matching requires the same key and exact modifiers", () => {
  const recording = parseShortcut("Ctrl+Shift+R");
  assert.ok(recording);
  assert.equal(matchesShortcut(recording, { key: "R", ctrlKey: true, shiftKey: true, altKey: false, metaKey: false }), true);
  assert.equal(matchesShortcut(recording, { key: "r", ctrlKey: true, shiftKey: false, altKey: false, metaKey: false }), false);
  assert.equal(matchesShortcut(recording, { key: "r", ctrlKey: true, shiftKey: true, altKey: true, metaKey: false }), false);
});

test("default zoom shortcuts match the shifted plus and unshifted minus keys", () => {
  assert.equal(matchesShortcut(parseShortcut(defaultShortcuts.zoomIn), {
    key: "+", ctrlKey: true, shiftKey: true, altKey: false, metaKey: false,
  }), true);
  assert.equal(matchesShortcut(parseShortcut(defaultShortcuts.zoomOut), {
    key: "-", ctrlKey: true, shiftKey: false, altKey: false, metaKey: false,
  }), true);
});

test("screenshot capture has a customizable default shortcut", () => {
  assert.ok(shortcutActions.some(({ id }) => id === "captureScreenshot"));
  assert.equal(defaultShortcuts.captureScreenshot, "Ctrl+Shift+S");
  assert.equal(matchesShortcut(parseShortcut(defaultShortcuts.captureScreenshot), {
    key: "S", ctrlKey: true, shiftKey: true, altKey: false, metaKey: false,
  }), true);
});

test("Space remains available for markers when the graph slider has focus", () => {
  assert.equal(allowsSpaceMarkerShortcut(" ", true), true);
  assert.equal(allowsSpaceMarkerShortcut(" ", false), false);
  assert.equal(allowsSpaceMarkerShortcut("Enter", true), false);
});

test("captured key events format space, plus, and modifier shortcuts", () => {
  assert.equal(shortcutFromEvent({ key: " ", ctrlKey: false, shiftKey: false, altKey: false, metaKey: false }), "Space");
  assert.equal(shortcutFromEvent({ key: "+", ctrlKey: true, shiftKey: true, altKey: false, metaKey: false }), "Ctrl+Shift+Plus");
  assert.equal(shortcutFromEvent({ key: "Control", ctrlKey: true, shiftKey: false, altKey: false, metaKey: false }), null);
});

test("saved bindings ignore invalid shortcuts and duplicate keys", () => {
  const restored = restoreShortcutBindings(JSON.stringify({
    toggleRecording: "Ctrl+Shift+R",
    togglePlayback: "Ctrl+Shift+R",
    insertMarker: "Not a shortcut",
  }), defaultShortcuts);

  assert.equal(restored.toggleRecording, "Ctrl+Shift+R");
  assert.equal(restored.togglePlayback, defaultShortcuts.togglePlayback);
  assert.equal(restored.insertMarker, defaultShortcuts.insertMarker);
});
