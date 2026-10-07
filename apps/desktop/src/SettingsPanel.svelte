<script lang="ts">
  import { onMount } from "svelte";
  import {
    allowsSpaceMarkerShortcut, defaultShortcuts, matchesShortcut, parseShortcut, restoreShortcutBindings, shortcutActions,
    shortcutFromEvent, type ShortcutAction, type ShortcutBindings,
  } from "./shortcuts";
  import type { WorkspaceTheme } from "./workspace";
  import type { AgentAccessStatus } from "./types";

  let {
    wheelScrubbing = $bindable(false),
    theme = $bindable<WorkspaceTheme>("midnight"),
    highContrast = $bindable(false),
    uiScale = $bindable(100),
    resetDisabled = false,
    onAction,
    onReset,
    diagnosticsOpen = false,
    onDiagnostics,
    agentAccessStatus = { configured: false, unlocked: false, host_connected: false, expires_at_unix_ms: null, active_operation: null, trace_gaps: 0 },
    onAgentConfigurePin,
    onAgentUnlock,
    onAgentLock,
    agentSetupJson = "",
  }: {
    wheelScrubbing?: boolean;
    theme?: WorkspaceTheme;
    highContrast?: boolean;
    uiScale?: number;
    resetDisabled?: boolean;
    onAction: (action: ShortcutAction) => void;
    onReset: () => void;
    diagnosticsOpen?: boolean;
    onDiagnostics: () => void;
    agentAccessStatus?: AgentAccessStatus;
    onAgentConfigurePin: (pin: string) => Promise<void>;
    onAgentUnlock: (pin: string) => Promise<void>;
    onAgentLock: () => Promise<void>;
    agentSetupJson?: string;
  } = $props();

  const shortcutsKey = "lognook.shortcuts";
  let shortcutBindings = $state<ShortcutBindings>({ ...defaultShortcuts });
  let capturingShortcut = $state<ShortcutAction | null>(null);
  let shortcutCaptureError = $state("");
  let scalePreview = $state(uiScale);
  let agentPin = $state("");
  let agentError = $state("");
  let agentBusy = $state(false);
  let agentSetupCopied = $state(false);

  $effect(() => { scalePreview = uiScale; });

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

  function previewScale(event: Event) {
    scalePreview = Number((event.currentTarget as HTMLInputElement).value);
  }

  function applyScale() {
    uiScale = scalePreview;
  }

  async function submitAgentPin() {
    agentBusy = true;
    agentError = "";
    try {
      if (agentAccessStatus.configured) await onAgentUnlock(agentPin);
      else await onAgentConfigurePin(agentPin);
      agentPin = "";
    } catch (error) {
      agentError = String(error);
    } finally {
      agentBusy = false;
    }
  }

  async function copyAgentSetup() {
    agentSetupCopied = false;
    try {
      await navigator.clipboard.writeText(agentSetupJson);
      agentSetupCopied = true;
      agentError = "";
    } catch {
      agentError = "Clipboard access failed. Copy the setup text from the details below.";
    }
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
    <button class="button secondary" type="button" aria-pressed={diagnosticsOpen} onclick={onDiagnostics}>{diagnosticsOpen ? "Close Diagnostics" : "Diagnostics"}</button>
    <section class="agent-access-settings" aria-label="Agent Access">
      <span class="settings-subtitle">AGENT ACCESS · {agentAccessStatus.unlocked ? agentAccessStatus.host_connected ? "CONNECTED" : "UNLOCKED" : "LOCKED"}</span>
      {#if agentAccessStatus.unlocked}
        <label><span>Change PIN</span><input class="text-input" type="password" inputmode="numeric" pattern="[0-9]{6}" maxlength="6" autocomplete="off" bind:value={agentPin} /></label>
        <button class="button secondary" type="button" disabled={agentBusy || !/^\d{6}$/.test(agentPin)} onclick={async () => { agentBusy = true; agentError = ""; try { await onAgentConfigurePin(agentPin); agentPin = ""; } catch (error) { agentError = String(error); } finally { agentBusy = false; } }}>Change PIN</button>
        <button class="button secondary" type="button" disabled={agentBusy} onclick={async () => { agentBusy = true; agentError = ""; try { await onAgentLock(); } catch (error) { agentError = String(error); } finally { agentBusy = false; } }}>Lock agent</button>
      {:else}
        <label><span>{agentAccessStatus.configured ? "PIN" : "Create six-digit PIN"}</span><input class="text-input" type="password" inputmode="numeric" pattern="[0-9]{6}" maxlength="6" autocomplete="off" bind:value={agentPin} /></label>
        <button class="button secondary" type="button" disabled={agentBusy || !/^\d{6}$/.test(agentPin)} onclick={submitAgentPin}>{agentAccessStatus.configured ? "Unlock agent" : "Set PIN"}</button>
      {/if}
      {#if agentAccessStatus.configured}<button class="button secondary" type="button" disabled={!agentSetupJson} onclick={copyAgentSetup}>{agentSetupCopied ? "Setup copied" : "Copy agent setup"}</button>{/if}
      {#if agentSetupJson}<details><summary>Setup text</summary><pre>{agentSetupJson}</pre></details>{/if}
      <small>Open LogNook and unlock Agent Access before the host starts this server. The PIN stays in this app.</small>
      {#if agentError}<small role="alert">{agentError}</small>{/if}
      {#if agentAccessStatus.unlocked && agentAccessStatus.expires_at_unix_ms}<small>Access expires after 15 minutes without agent activity.</small>{/if}
      {#if agentAccessStatus.active_operation}<small>Active operation: {agentAccessStatus.active_operation}</small>{/if}
      {#if agentAccessStatus.trace_gaps > 0}<small role="status">Communication trace gaps: {agentAccessStatus.trace_gaps}</small>{/if}
    </section>
    <label class="settings-theme">
      <span>Color theme</span>
      <select class="text-input settings-theme-select" aria-label="Color theme" value={theme} onchange={(event) => theme = event.currentTarget.value as WorkspaceTheme}>
        <option value="midnight">Midnight</option>
        <option value="slate-lift">Slate Lift</option>
        <option value="steel">Steel</option>
        <option value="mist">Mist</option>
      </select>
    </label>
    <label><input type="checkbox" checked={highContrast} onchange={(event) => highContrast = event.currentTarget.checked} /> High contrast</label>
    <label class="settings-scale">
      <span>UI scale <strong>{scalePreview}%</strong></span>
      <input type="range" min="80" max="120" step="5" aria-label="UI scale" aria-valuetext={`${scalePreview}%`} value={scalePreview} oninput={previewScale} onchange={applyScale} />
    </label>
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
