<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import { onMount } from "svelte";
  import type { RecordingInfo } from "./types";
  import {
    formatRecordingTime,
    openAndActivateRecording,
    recentRecordingsFirst,
    type BrowserRecording,
    type BrowserRecordingRow,
  } from "./sessionBrowser";

  let {
    disabled = false,
    onOpen,
  }: {
    disabled?: boolean;
    onOpen: (info: RecordingInfo) => void | Promise<void>;
  } = $props();

  let recordings = $state<BrowserRecording[]>([]);
  let loading = $state(false);
  let openingPath = $state<string | null>(null);
  let error = $state("");

  onMount(() => { void refresh(); });

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

  async function openRecording(recording: BrowserRecording) {
    if (disabled || openingPath !== null || recording.sample_count === 0) return;
    openingPath = recording.path;
    error = "";
    try {
      await openAndActivateRecording(
        () => invoke<RecordingInfo>("open_recording", { path: recording.path }),
        onOpen,
      );
    } catch (cause) {
      error = errorMessage(cause);
    } finally {
      openingPath = null;
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
          <button
            class="button secondary"
            type="button"
            aria-label={`Open ${recording.name}`}
            title={recording.sample_count === 0 ? "Recording contains no samples" : undefined}
            onclick={() => openRecording(recording)}
            disabled={disabled || openingPath !== null || recording.sample_count === 0}
          >
            {openingPath === recording.path ? "Opening…" : "Open"}
          </button>
        </li>
      {/each}
    </ul>
  {/if}
</section>

<style>
  .session-browser { display: grid; gap: 0.65rem; }
  .browser-heading, .recording-row, .recording-details { display: flex; align-items: center; }
  .browser-heading, .recording-row { justify-content: space-between; gap: 0.75rem; }
  .browser-heading .eyebrow { margin: 0 0 0.2rem; }
  .recording-list { display: grid; gap: 0.35rem; list-style: none; margin: 0; padding: 0; }
  .recording-row { border-top: 1px solid var(--line, #293847); padding-top: 0.55rem; }
  .recording-details { flex-wrap: wrap; gap: 0.3rem 0.8rem; min-width: 0; }
  .recording-details strong { flex-basis: 100%; overflow-wrap: anywhere; }
  .recording-details span, .browser-message { color: var(--muted, #9aa8b5); font-size: 0.86rem; }
  .browser-message { margin: 0; }
  .recording-row .button { flex: 0 0 auto; }
</style>
