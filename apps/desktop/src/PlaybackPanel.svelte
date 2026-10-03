<script lang="ts">
  import type { Marker, PlaybackState, RecordingInfo } from "./types";
  import { formatDurationNs, formatTimelineNs, type TimeWindow } from "./workspace";

  let {
    info,
    playback,
    markers,
    range,
    openDisabled = false,
    disabled,
    onOpen,
    onClose,
    onTogglePlay,
    onStart,
    onScrub,
    onSpeed,
    onAddMarker,
    onUpdateMarker,
    onDeleteMarker,
    onJumpMarker,
    onZoom,
    onPan,
    onReset,
  }: {
    info: RecordingInfo | null;
    playback: PlaybackState;
    markers: Marker[];
    range: TimeWindow;
    openDisabled?: boolean;
    disabled: boolean;
    onOpen: () => void;
    onClose: () => void;
    onTogglePlay: () => void;
    onStart: () => void;
    onScrub: (positionNs: number) => void;
    onSpeed: (speed: number) => void;
    onAddMarker: (label: string, note: string) => void;
    onUpdateMarker: (id: number, label: string, note: string) => void;
    onDeleteMarker: (id: number) => void;
    onJumpMarker: (positionNs: number) => void;
    onZoom: (factor: number) => void;
    onPan: (fraction: number) => void;
    onReset: () => void;
  } = $props();

  let seekDraft = $state(0);
  let seeking = $state(false);
  let markerLabel = $state("");
  let markerNote = $state("");
  let editingId = $state<number | null>(null);
  let editingLabel = $state("");
  let editingNote = $state("");
  $effect(() => { if (!seeking) seekDraft = playback.position_ns; });

  function addMarker() {
    onAddMarker(markerLabel.trim(), markerNote.trim());
    markerLabel = "";
    markerNote = "";
  }

  function edit(marker: Marker) {
    if (marker.id === null) return;
    editingId = marker.id;
    editingLabel = marker.label;
    editingNote = marker.note;
  }

  function saveEdit() {
    if (editingId === null) return;
    onUpdateMarker(editingId, editingLabel.trim(), editingNote.trim());
    editingId = null;
  }
</script>

<section class="panel playback-panel" aria-label="Recording player">
  <div class="playback-main-row">
    <div class="player-identity">
      <p class="eyebrow">SESSION PLAYER</p>
      <strong>{info ? `${info.sample_count.toLocaleString()} samples` : "No session loaded"}</strong>
    </div>
    <button class="button secondary" onclick={onOpen} disabled={disabled || openDisabled}>Open file</button>
    {#if info}
      <button class="button secondary close-session" onclick={onClose} disabled={disabled}>Close session</button>
      <div class="playback-transport">
        <button class="button secondary transport-button" aria-label="Return to start" title="Return to start" onclick={onStart} disabled={disabled}>|◀</button>
        <button class="button primary transport-button" aria-label={playback.playing ? "Pause" : "Play"} onclick={onTogglePlay} disabled={disabled}>{playback.playing ? "Ⅱ" : "▶"}</button>
      </div>
      <span class="playback-time">{formatDurationNs(playback.position_ns)} <span>/</span> {formatDurationNs(playback.duration_ns)}</span>
      <select class="text-input speed-select" aria-label="Playback speed" value={playback.speed} onchange={(event) => onSpeed(Number(event.currentTarget.value))} disabled={disabled}>
        {#each [0.1, 0.25, 0.5, 1, 2, 4, 8, 16, 32] as speed}
          <option value={speed}>{speed}×</option>
        {/each}
      </select>
    {/if}
  </div>

  {#if info}
    <div class="timeline-row">
      <span class="timeline-start">{formatTimelineNs(range.startNs)}</span>
      <div class="seek-wrap">
        <div class="seek-marker-ticks" aria-hidden="true">
          {#each markers as marker (marker.id ?? `${marker.elapsed_ns}-${marker.label}`)}
            {#if marker.elapsed_ns >= 0 && marker.elapsed_ns <= playback.duration_ns}
              <span style={`left:${playback.duration_ns > 0 ? (marker.elapsed_ns / playback.duration_ns) * 100 : 0}%`}></span>
            {/if}
          {/each}
        </div>
        <input
          class="timeline-slider"
          type="range"
          min="0"
          max={Math.max(playback.duration_ns, 1)}
          step="1000000"
          value={seeking ? seekDraft : playback.position_ns}
          aria-label="Recording position"
          disabled={disabled}
          onpointerdown={() => seeking = true}
          oninput={(event) => { seeking = true; seekDraft = Number(event.currentTarget.value); onScrub(seekDraft); }}
          onchange={() => seeking = false}
          onpointercancel={() => seeking = false}
        />
      </div>
      <span class="timeline-end">{formatTimelineNs(range.endNs)}</span>
      <div class="viewport-controls" role="group" aria-label="Graph time range">
        <button class="icon-button" title="Pan earlier" aria-label="Pan earlier" onclick={() => onPan(-0.25)} disabled={disabled}>←</button>
        <button class="icon-button" title="Zoom out" aria-label="Zoom out" onclick={() => onZoom(0.5)} disabled={disabled}>−</button>
        <button class="icon-button" title="Reset zoom" aria-label="Reset zoom" onclick={onReset} disabled={disabled}>↺</button>
        <button class="icon-button" title="Zoom in" aria-label="Zoom in" onclick={() => onZoom(2)} disabled={disabled}>＋</button>
        <button class="icon-button" title="Pan later" aria-label="Pan later" onclick={() => onPan(0.25)} disabled={disabled}>→</button>
      </div>
    </div>

    <div class="marker-workspace">
      <form class="marker-create" onsubmit={(event) => { event.preventDefault(); addMarker(); }}>
        <input class="text-input marker-label-input" bind:value={markerLabel} placeholder="Marker label" aria-label="Marker label" disabled={disabled} />
        <input class="text-input marker-note-input" bind:value={markerNote} placeholder="Note (optional)" aria-label="Marker note" disabled={disabled} />
        <button class="button secondary" type="submit" disabled={disabled}>＋ Marker</button>
      </form>
      <div class="marker-list" aria-label="Session markers">
        {#each markers as marker (marker.id ?? `${marker.elapsed_ns}-${marker.label}`)}
          {#if editingId === marker.id}
            <div class="marker-item editing">
              <span class="marker-time">{formatDurationNs(marker.elapsed_ns)}</span>
              <input class="text-input" bind:value={editingLabel} aria-label="Edit marker label" />
              <input class="text-input" bind:value={editingNote} aria-label="Edit marker note" />
              <button class="icon-button" aria-label="Save marker" onclick={saveEdit}>✓</button>
              <button class="icon-button" aria-label="Cancel edit" onclick={() => editingId = null}>×</button>
            </div>
          {:else}
            <div class="marker-item">
              <button class="marker-jump" onclick={() => onJumpMarker(marker.elapsed_ns)} title={marker.note || marker.label}>
                <span class="marker-time">{formatDurationNs(marker.elapsed_ns)}</span><strong>{marker.label || "Marker"}</strong>
              </button>
              {#if marker.id !== null}
                <button class="icon-button" aria-label={`Edit ${marker.label}`} onclick={() => edit(marker)}>✎</button>
                <button class="icon-button" aria-label={`Delete ${marker.label}`} onclick={() => onDeleteMarker(marker.id!)}>×</button>
              {/if}
            </div>
          {/if}
        {:else}
          <span class="marker-empty">No markers</span>
        {/each}
      </div>
    </div>
  {/if}
</section>
