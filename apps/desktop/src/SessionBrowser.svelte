<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import {
    formatRecordingTime,
    recentRecordingsFirst,
    type BrowserRecording,
    type BrowserRecordingRow,
  } from "./sessionBrowser";

  let {
    open = false,
    disabled = false,
    openDisabled = false,
    onOpen,
    onShare,
    onSaveAs,
  }: {
    open?: boolean;
    disabled?: boolean;
    openDisabled?: boolean;
    onOpen: (path: string) => void | Promise<void>;
    onShare: (path: string) => void | Promise<void>;
    onSaveAs: (path: string) => void | Promise<void>;
  } = $props();

  let recordings = $state<BrowserRecording[]>([]);
  let loading = $state(false);
  let actionPath = $state<string | null>(null);
  let actionKind = $state<"open" | "share" | "saveAs" | null>(null);
  let error = $state("");
  let wasOpen = false;

  $effect(() => {
    const justOpened = open && !wasOpen;
    wasOpen = open;
    if (justOpened) queueMicrotask(() => { if (open) void refresh(); });
  });

  async function refresh() {
    if (loading) return;
    loading = true;
    error = "";
    try {
      const rows = await invoke<BrowserRecordingRow[]>("list_recordings");
      recordings = recentRecordingsFirst(rows.map(([path, name, modified_unix_ms, sample_count]) => ({
        path,
        name,
        modified_unix_ms,
        sample_count,
      })));
    } catch (cause) {
      error = errorMessage(cause);
    } finally {
      loading = false;
    }
  }

  async function runRecordingAction(recording: BrowserRecording, action: "open" | "share" | "saveAs") {
    if (disabled || loading || actionPath !== null) return;
    if (action === "open" && (openDisabled || recording.sample_count === 0)) return;
    actionPath = recording.path;
    actionKind = action;
    error = "";
    try {
      if (action === "open") {
        await onOpen(recording.path);
      } else if (action === "share") await onShare(recording.path);
      else await onSaveAs(recording.path);
    } catch (cause) {
      error = errorMessage(cause);
    } finally {
      actionPath = null;
      actionKind = null;
    }
  }

  function errorMessage(cause: unknown): string {
    if (typeof cause === "string") return cause;
    if (cause instanceof Error) return cause.message;
    return "The saved recordings could not be loaded.";
  }
</script>

<section class="panel session-browser" aria-label="Saved recordings">
  <div class="browser-heading">
    <div>
      <p class="eyebrow">RECENT RECORDINGS</p>
      <strong>{recordings.length.toLocaleString()} saved</strong>
    </div>
    <button class="button secondary" type="button" onclick={refresh} disabled={disabled || loading}>
      {loading ? "Refreshing…" : "Refresh"}
    </button>
  </div>

  {#if error}
    <p class="browser-message" role="alert">{error}</p>
  {:else if loading && recordings.length === 0}
    <p class="browser-message" aria-live="polite">Loading recordings…</p>
  {:else if recordings.length === 0}
    <p class="browser-message">No saved recordings</p>
  {:else}
    <ul class="recording-list">
      {#each recordings as recording (recording.path)}
        <li class="recording-row">
          <div class="recording-details">
            <strong>{recording.name}</strong>
            <span>{formatRecordingTime(recording.modified_unix_ms)}</span>
            <span>{recording.sample_count.toLocaleString()} samples</span>
          </div>
          <div class="recording-actions">
            <button
              class="button secondary"
              type="button"
              aria-label={`Open ${recording.name}`}
              title={recording.sample_count === 0 ? "Recording contains no samples" : undefined}
              onclick={() => runRecordingAction(recording, "open")}
              disabled={disabled || loading || openDisabled || actionPath !== null || recording.sample_count === 0}
            >
              {#if actionPath === recording.path && actionKind === "open"}Opening…{:else}Open{/if}
            </button>
            <button
              class="button secondary"
              type="button"
              aria-label={`Share ${recording.name} as CSV`}
              title="Save a CSV copy with samples and markers for sharing"
              onclick={() => runRecordingAction(recording, "share")}
              disabled={disabled || loading || actionPath !== null}
            >
              {#if actionPath === recording.path && actionKind === "share"}Sharing…{:else}Share{/if}
            </button>
            <button
              class="button secondary"
              type="button"
              aria-label={`Save ${recording.name} as a copy`}
              title="Save a copy of the native .lognook session"
              onclick={() => runRecordingAction(recording, "saveAs")}
              disabled={disabled || loading || actionPath !== null}
            >
              {#if actionPath === recording.path && actionKind === "saveAs"}Saving…{:else}Save As{/if}
            </button>
          </div>
        </li>
      {/each}
    </ul>
  {/if}
</section>

<style>
  .session-browser { display: grid; gap: 0.65rem; padding: 9px; }
  .browser-heading, .recording-row, .recording-details, .recording-actions { display: flex; align-items: center; }
  .browser-heading, .recording-row { justify-content: space-between; gap: 0.75rem; }
  .browser-heading .eyebrow { margin: 0 0 0.2rem; }
  .recording-list { display: grid; gap: 0.35rem; list-style: none; margin: 0; padding: 0; }
  .recording-row { border-top: 1px solid var(--line, #293847); padding-top: 0.55rem; }
  .recording-row { flex-wrap: wrap; }
  .recording-details { flex: 1; flex-wrap: wrap; gap: 0.3rem 0.8rem; min-width: 0; }
  .recording-details strong { flex-basis: 100%; overflow-wrap: anywhere; }
  .recording-details span, .browser-message { color: var(--muted, #9aa8b5); font-size: 0.86rem; }
  .browser-message { margin: 0; }
  .recording-actions { flex-wrap: wrap; gap: 0.35rem; }
  .recording-actions .button { flex: 0 0 auto; }
</style>
