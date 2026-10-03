<script lang="ts">
  import { onMount } from "svelte";
  import {
    allowsSpaceMarkerShortcut, defaultShortcuts, matchesShortcut, parseShortcut, restoreShortcutBindings, shortcutActions,
    shortcutFromEvent, type ShortcutAction, type ShortcutBindings,
  } from "./shortcuts";

  let {
    wheelScrubbing = $bindable(false),
    resetDisabled = false,
    onAction,
    onReset,
  }: {
    wheelScrubbing?: boolean;
    resetDisabled?: boolean;
    onAction: (action: ShortcutAction) => void;
    onReset: () => void;
  } = $props();

  const shortcutsKey = "lognook.shortcuts";
  let shortcutBindings = $state<ShortcutBindings>({ ...defaultShortcuts });
  let capturingShortcut = $state<ShortcutAction | null>(null);
  let shortcutCaptureError = $state("");

  onMount(() => {
    let saved: string | null = null;
    try {
      saved = localStorage.getItem(shortcutsKey);
      wheelScrubbing = localStorage.getItem("lognook.wheel-scrubbing") === "true";
    } catch { /* Preferences are optional. */ }
    shortcutBindings = restoreShortcutBindings(saved, defaultShortcuts);
    window.addEventListener("keydown", handleGlobalKeydown);
    return () => window.removeEventListener("keydown", handleGlobalKeydown);
  });

  function shortcutSignature(value: string | null): string | null {
    const shortcut = value ? parseShortcut(value) : null;
    return shortcut ? [shortcut.key, shortcut.ctrl, shortcut.shift, shortcut.alt, shortcut.meta].join("|") : null;
  }

  function setShortcutBinding(action: ShortcutAction, value: string | null) {
    const signature = shortcutSignature(value);
    if (signature && shortcutActions.some(({ id }) => id !== action && shortcutSignature(shortcutBindings[id]) === signature)) {
      shortcutCaptureError = "That shortcut is already assigned.";
      return;
    }
    shortcutBindings = { ...shortcutBindings, [action]: value };
    shortcutCaptureError = "";
    try { localStorage.setItem(shortcutsKey, JSON.stringify(shortcutBindings)); } catch { /* Preferences are optional. */ }
  }

  function isBlockedTarget(target: EventTarget | null): boolean {
    return target instanceof Element
      && Boolean(target.closest('input, textarea, select, button, a[href], summary, [contenteditable="true"], [role="textbox"], [role="slider"]'));
  }

  function handleGlobalKeydown(event: KeyboardEvent) {
    if (event.repeat) return;
    if (capturingShortcut) {
      const active = document.activeElement;
      if (!(active instanceof HTMLElement) || active.dataset.shortcutCapture !== "true") return;
      if (event.key === "Escape") {
        event.preventDefault();
        capturingShortcut = null;
        shortcutCaptureError = "";
        return;
      }
      const shortcut = shortcutFromEvent(event);
      if (!shortcut) return;
      event.preventDefault();
      const action = capturingShortcut;
      setShortcutBinding(action, shortcut);
      if (!shortcutCaptureError) capturingShortcut = null;
      return;
    }
    const graphFocused = [event.target, document.activeElement].some((target) =>
      target instanceof Element && Boolean(target.closest('.chart-interaction[role="slider"]')));
    const spaceMarker = allowsSpaceMarkerShortcut(event.key, graphFocused);
    if (!spaceMarker && (isBlockedTarget(event.target) || isBlockedTarget(document.activeElement))) return;

    const match = shortcutActions.find(({ id }) => {
      const binding = shortcutBindings[id];
      const shortcut = binding ? parseShortcut(binding) : null;
      return shortcut !== null && matchesShortcut(shortcut, event);
    });
    if (!match) return;
    event.preventDefault();
    onAction(match.id);
  }

  function updateWheelScrubbing(event: Event) {
    wheelScrubbing = (event.currentTarget as HTMLInputElement).checked;
    try { localStorage.setItem("lognook.wheel-scrubbing", String(wheelScrubbing)); } catch { /* Preferences are optional. */ }
  }

  function reset() {
    shortcutBindings = { ...defaultShortcuts };
    capturingShortcut = null;
    shortcutCaptureError = "";
    try { localStorage.removeItem(shortcutsKey); } catch { /* Preferences are optional. */ }
    onReset();
  }
</script>

<details class="settings-menu">
  <summary class="icon-button" aria-label="Settings" title="Settings">⚙</summary>
  <div class="settings-popover">
    <strong>Settings</strong>
    <label><input type="checkbox" checked={wheelScrubbing} onchange={updateWheelScrubbing} /> Wheel scrubs graph</label>
    <small>Replay keeps the playhead centered while playing.</small>
    <section class="shortcut-settings" aria-label="Keyboard shortcuts">
      <span class="settings-subtitle">KEYBOARD SHORTCUTS</span>
      {#each shortcutActions as action (action.id)}
        <div class="shortcut-row">
          <span>{action.label}</span>
          <button
            type="button"
            class="shortcut-binding"
            class:capturing={capturingShortcut === action.id}
            data-shortcut-capture="true"
            aria-label={`Set shortcut for ${action.label}`}
            aria-pressed={capturingShortcut === action.id}
            onclick={() => { capturingShortcut = action.id; shortcutCaptureError = ""; }}
          >{capturingShortcut === action.id ? "Press keys…" : shortcutBindings[action.id] ?? "Unassigned"}</button>
          <button type="button" class="shortcut-clear" aria-label={`Clear ${action.label} shortcut`} onclick={() => setShortcutBinding(action.id, null)}>×</button>
        </div>
      {/each}
      {#if shortcutCaptureError}<small class="shortcut-error" role="alert">{shortcutCaptureError}</small>{/if}
      <small>Select a shortcut, then press a key combination. Escape cancels.</small>
    </section>
    <button class="button secondary" onclick={reset} disabled={resetDisabled}>Reset preferences</button>
  </div>
</details>
