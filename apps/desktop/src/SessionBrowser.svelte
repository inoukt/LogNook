<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import {
    formatRecordingTime,
    recentRecordingsFirst,
    recordingFromRow,
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
  let actionKind = $state<"open" | "share" | "saveAs" | "rename" | "delete" | "folder" | null>(null);
  let renamingPath = $state<string | null>(null);
  let renameName = $state("");
  let confirmDeletePath = $state<string | null>(null);
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
      recordings = recentRecordingsFirst(rows.map(recordingFromRow));
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

  function startRename(recording: BrowserRecording) {
    renamingPath = recording.path;
    renameName = recording.name;
    confirmDeletePath = null;
  }

  function focusRenameInput(input: HTMLInputElement) {
    input.focus();
    input.select();
  }

  async function manageRecording(recording: BrowserRecording, action: "rename" | "delete" | "folder") {
    if (disabled || loading || actionPath !== null) return;
    if (action === "delete" && confirmDeletePath !== recording.path) return;
    actionPath = recording.path;
    actionKind = action;
    error = "";
    try {
      if (action === "rename") await invoke("rename_recording", { path: recording.path, name: renameName });
      else if (action === "delete") await invoke("delete_recording", { path: recording.path });
      else await invoke("open_recording_folder", { path: recording.path });
      renamingPath = null;
      confirmDeletePath = null;
      if (action !== "folder") await refresh();
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
            {#if renamingPath === recording.path}
              <form class="rename-form" onsubmit={(event) => { event.preventDefault(); void manageRecording(recording, "rename"); }}>
                <input use:focusRenameInput aria-label={`New filename for ${recording.name}`} bind:value={renameName} required maxlength="180" disabled={disabled || actionPath !== null} />
                <button class="button secondary" type="submit" disabled={disabled || loading || actionPath !== null}>Save name</button>
                <button class="button secondary" type="button" onclick={() => { renamingPath = null; }} disabled={actionPath !== null}>Cancel</button>
              </form>
            {:else}
              <strong>{recording.name}</strong>
            {/if}
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
              aria-label={`Share ${recording.name}`}
              title="Create a public session link or save a CSV locally"
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
            <button class="button secondary" type="button" aria-label={`Open folder for ${recording.name}`} onclick={() => manageRecording(recording, "folder")} disabled={disabled || loading || actionPath !== null}>
              {#if actionPath === recording.path && actionKind === "folder"}Opening…{:else}Folder{/if}
            </button>
            {#if confirmDeletePath === recording.path}
              <button class="button secondary" type="button" onclick={() => manageRecording(recording, "delete")} disabled={disabled || loading || actionPath !== null}>
                {#if actionPath === recording.path && actionKind === "delete"}Deleting…{:else}Confirm delete{/if}
              </button>
              <button class="button secondary" type="button" onclick={() => { confirmDeletePath = null; }} disabled={actionPath !== null}>Cancel</button>
            {:else}
              <button class="button secondary" type="button" aria-label={`Rename ${recording.name}`} onclick={() => startRename(recording)} disabled={disabled || loading || actionPath !== null}>Rename</button>
              <button class="button secondary" type="button" aria-label={`Delete ${recording.name}`} onclick={() => { confirmDeletePath = recording.path; renamingPath = null; }} disabled={disabled || loading || actionPath !== null}>Delete</button>
            {/if}
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
  .rename-form { display: flex; flex-basis: 100%; flex-wrap: wrap; gap: 0.35rem; }
  .rename-form input { min-width: 8rem; flex: 1; }
  .recording-details span, .browser-message { color: var(--muted, #9aa8b5); font-size: 0.86rem; }
  .browser-message { margin: 0; }
  .recording-actions { flex-wrap: wrap; gap: 0.35rem; }
  .recording-actions .button { flex: 0 0 auto; }
</style>
