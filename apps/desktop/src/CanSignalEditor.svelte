<script lang="ts">
  import { onDestroy, onMount } from "svelte";
  import { invoke } from "@tauri-apps/api/core";
  import { canHelpBubblesEnabled } from "./canHelp";
  import { canSignalBitPositions, canSignalDefinitionError, extractCanSignalRaw } from "./canWorkbench";
  import { canScopePoints, scopePath } from "./canDiscovery";
  import type { CanFrame, CanSignalDefinition, Sample } from "./types";

  export let frame: CanFrame;
  export let fields: CanSignalDefinition[] = [];
  export let latestElapsedNs = frame.elapsed_ns;
  export let initialFieldId: string | null = null;
  export let initialStartBit = 0;
  export let initialBitLength = 16;
  export let initialName = "";
  export let evidence = "";
  export let history: CanFrame[] = [];
  export let onSave: (field: CanSignalDefinition) => unknown = () => {};
  export let onRemove: (id: string) => unknown = () => {};
  export let onClose: () => void = () => {};

  const bytes = Array.from({ length: 8 }, (_, index) => index);
  const bitRows = [7, 6, 5, 4, 3, 2, 1, 0];
  const initialField = fields.find((field) => field.id === initialFieldId
    && field.can_id === frame.can_id && field.extended === frame.extended);
  let selectedFieldId = initialField?.id ?? "__new";
  let fieldId = initialField?.id ?? makeFieldId();
  let name = initialField?.name ?? (initialName || `CAN 0x${frame.can_id.toString(16).toUpperCase()}`);
  let startBit = initialField?.start_bit ?? initialStartBit;
  let bitLength = initialField?.bit_length ?? initialBitLength;
  let byteOrder: "intel" | "motorola" = initialField?.byte_order ?? "intel";
  let signed = initialField?.signed ?? false;
  let factor = initialField?.factor ?? 1;
  let offset = initialField?.offset ?? 0;
  let unit = initialField?.unit ?? "";
  let preview: Sample | null = null;
  let previewError = "";
  let dialogElement: HTMLDialogElement;
  let previewTimer: ReturnType<typeof setTimeout> | undefined;
  let previewSequence = 0;
  let previewInFlight = false;
  let previewPending = false;
  let followLive = true;
  const evidenceFrame = frame;
  let previewFrame: CanFrame | null = null;
  let lastLiveFrame: CanFrame | null = null;
  $: liveFrame = [...history].reverse().find((item) => item.direction === "rx" && item.can_id === evidenceFrame.can_id && item.extended === evidenceFrame.extended);
  $: frame = followLive && liveFrame ? liveFrame : evidenceFrame;
  $: refreshFramePreview(frame);
  $: scope = canScopePoints(history, frame.can_id, frame.extended, startBit, bitLength, byteOrder, signed, factor, offset);

  $: matchingFields = fields.filter((field) => field.can_id === frame.can_id && field.extended === frame.extended);
  $: selectedBits = canSignalBitPositions(startBit, bitLength, byteOrder) ?? [];
  $: rawValue = extractCanSignalRaw(frame.data, startBit, bitLength, byteOrder, signed);
  $: previewRawValue = preview && previewFrame
    ? extractCanSignalRaw(previewFrame.data, startBit, bitLength, byteOrder, signed)
    : rawValue;
  $: frameAgeNs = Math.max(0, latestElapsedNs - frame.elapsed_ns);
  $: definitionError = canSignalDefinitionError({
    id: fieldId.trim(), name: name.trim(), can_id: frame.can_id, extended: frame.extended,
    start_bit: startBit, bit_length: bitLength, byte_order: byteOrder, signed, factor, offset, unit: unit.trim(),
  }) ?? (fields.some((field) => field.id === fieldId.trim() && field.id !== selectedFieldId) ? "Signal ID already exists" : null);

  function makeFieldId() {
    const base = `can:${frame.can_id.toString(16)}`;
    let id = base;
    let suffix = 2;
    while (fields.some((field) => field.id === id)) id = `${base}-${suffix++}`;
    return id;
  }

  function definition(): CanSignalDefinition {
    return {
      id: fieldId.trim(), name: name.trim(), can_id: frame.can_id, extended: frame.extended,
      start_bit: startBit, bit_length: bitLength, byte_order: byteOrder, signed,
      factor, offset, unit: unit.trim(),
    };
  }

  function closeEditor() {
    if (dialogElement?.open) dialogElement.close();
    onClose();
  }

  function chooseField() {
    const field = matchingFields.find(({ id }) => id === selectedFieldId);
    if (field) {
      fieldId = field.id;
      name = field.name;
      startBit = field.start_bit;
      bitLength = field.bit_length;
      byteOrder = field.byte_order;
      signed = field.signed;
      factor = field.factor;
      offset = field.offset;
      unit = field.unit;
    } else {
      selectedFieldId = "__new";
      fieldId = makeFieldId();
      name = `CAN 0x${frame.can_id.toString(16).toUpperCase()}`;
      startBit = 0;
      bitLength = 16;
      byteOrder = "intel";
      signed = false;
      factor = 1;
      offset = 0;
      unit = "";
    }
    schedulePreview();
  }

  function selectStart(bit: number) {
    startBit = bit;
    schedulePreview();
  }

  function alignLength(length: number) {
    const byte = Math.floor(startBit / 8);
    startBit = byte * 8 + (byteOrder === "motorola" ? 7 : 0);
    bitLength = length;
    schedulePreview();
  }

  function schedulePreview() {
    schedulePreviewTask(true);
  }

  function schedulePreviewTask(invalidate: boolean) {
    if (previewTimer) clearTimeout(previewTimer);
    if (invalidate) previewSequence++;
    const sequence = previewSequence;
    if (invalidate) {
      preview = null;
      previewFrame = null;
    }
    previewError = "";
    previewTimer = setTimeout(async () => {
      previewTimer = undefined;
      if (previewInFlight) { previewPending = true; return; }
      previewInFlight = true;
      const requestFrame = frame;
      const current = definition();
      const invalid = canSignalDefinitionError(current)
        ?? (fields.some((field) => field.id === current.id && field.id !== selectedFieldId) ? "Signal ID already exists" : null);
      try {
        const requestRaw = extractCanSignalRaw(requestFrame.data, startBit, bitLength, byteOrder, signed);
        if (invalid) {
          if (sequence === previewSequence) previewError = invalid;
        } else if (requestRaw === null) {
          if (sequence === previewSequence) previewError = `Selected ${bitLength}-bit field extends past this ${requestFrame.data.length}-byte frame.`;
        } else {
          const result = await invoke<Sample | null>("preview_can_signal", { frame: requestFrame, definition: current });
          if (sequence === previewSequence) {
            preview = result;
            previewFrame = requestFrame;
            if (!result) previewError = "This frame is not an RX sample for the selected CAN ID.";
          }
        }
      } catch (error) {
        if (sequence === previewSequence) previewError = String(error);
      } finally {
        previewInFlight = false;
        if (previewPending) {
          previewPending = false;
          schedulePreviewTask(false);
        }
      }
    }, 120);
  }

  function refreshFramePreview(current: CanFrame) {
    if (current === lastLiveFrame) return;
    lastLiveFrame = current;
    if (previewInFlight) previewPending = true;
    else if (!previewTimer) schedulePreviewTask(false);
  }

  async function save() {
    const field = definition();
    if (definitionError) { previewError = definitionError; return; }
    try { await onSave(field); closeEditor(); }
    catch (error) { previewError = String(error); }
  }

  async function remove() {
    if (selectedFieldId === "__new") return;
    try { await onRemove(selectedFieldId); closeEditor(); }
    catch (error) { previewError = String(error); }
  }

  onMount(() => { dialogElement.showModal(); schedulePreview(); });
  onDestroy(() => { if (previewTimer) clearTimeout(previewTimer); });
</script>

<dialog bind:this={dialogElement} class="can-signal-editor" aria-labelledby="can-editor-title" oncancel={(event) => { event.preventDefault(); closeEditor(); }}>
    <header class="can-editor-heading">
      <div><p class="eyebrow">VISUAL CAN FIELD</p><h2 id="can-editor-title">{selectedFieldId === "__new" ? "Add signal" : "Edit signal"}</h2></div>
      <button class="button secondary" type="button" onclick={closeEditor} aria-label="Close CAN signal editor" title={$canHelpBubblesEnabled ? "Close the editor without saving current changes." : undefined}>×</button>
    </header>
    <p class="can-editor-frame">{followLive ? "Live RX" : "Frozen RX"} · {frame.extended ? "29-bit" : "11-bit"} ID 0x{frame.can_id.toString(16).toUpperCase().padStart(frame.extended ? 8 : 3, "0")} · DLC {frame.data.length} · {(frame.elapsed_ns / 1e9).toFixed(3)}s{#if frameAgeNs > 0} · { (frameAgeNs / 1e9).toFixed(2)}s older than latest bus frame{/if}</p>
    {#if matchingFields.length}
      <label class="can-editor-existing">Field
        <select class="text-input" bind:value={selectedFieldId} onchange={chooseField} title={$canHelpBubblesEnabled ? "Choose a saved field for this CAN ID or start a new definition." : undefined}>
          <option value="__new">＋ New field</option>
          {#each matchingFields as field (field.id)}<option value={field.id}>{field.name} · {field.id}</option>{/each}
        </select>
      </label>
    {/if}

    <div class="can-bit-grid" aria-label="Select signal start bit; active cells show the extraction range" title={$canHelpBubblesEnabled ? "Bit numbers are LSB0 within each byte. Click a cell to choose the start bit; highlighted cells show the selected field range." : undefined}>
      {#each bytes as byte}
        <div class="can-bit-byte">
          <strong>B{byte} {frame.data[byte] === undefined ? "—" : `0x${frame.data[byte].toString(16).padStart(2, "0").toUpperCase()}`}</strong>
          {#each bitRows as bitRow}
            {@const bit = byte * 8 + bitRow}
            <button type="button" class:in-range={selectedBits.includes(bit)} class:start-bit={startBit === bit} aria-pressed={selectedBits.includes(bit)} aria-label={`Byte ${byte}, bit ${bitRow}, ${frame.data[byte] === undefined ? "no data" : (frame.data[byte] >> bitRow) & 1}`} onclick={() => selectStart(bit)} title={$canHelpBubblesEnabled ? `Choose byte ${byte}, bit ${bitRow} (LSB0 position ${bit}) as the signal start.` : undefined}>
              <small>{bitRow}</small><span>{frame.data[byte] === undefined ? "·" : (frame.data[byte] >> bitRow) & 1}</span>
            </button>
          {/each}
        </div>
      {/each}
    </div>
    <p class="can-editor-help">Bit numbers are LSB0 within each byte. Click a start bit; the highlighted range follows the selected Intel or Motorola order.</p>
    {#if evidence}<p class="can-editor-help" title={$canHelpBubblesEnabled ? "Raw byte transitions from the marked event; they are correlation evidence, not a decoded signal." : undefined}>Event evidence: {evidence}</p>{/if}
    <label class="can-editor-check" title={$canHelpBubblesEnabled ? "Use the latest matching RX frame for the byte grid and decoded preview; off keeps the frame that opened this editor. The scope still shows recent samples." : undefined}><input type="checkbox" bind:checked={followLive} /> Follow live RX · {followLive ? "latest frame" : "event frame frozen"}</label>
    <svg class="can-mini-scope" viewBox="0 0 100 50" preserveAspectRatio="none" role="img" aria-label="Recent decoded CAN field values">{#if $canHelpBubblesEnabled}<title>Draft values from up to 120 recent RX frames for this ID that fit the selected field; this scope is independent of Follow live.</title>{/if}<path d={scopePath(scope)} fill="none" stroke="var(--accent)" stroke-width="1.5" vector-effect="non-scaling-stroke" /></svg>
    <p class="can-editor-help">{scope.length} recent samples · preview only; saving uses the Rust decoder and common recording pipeline.</p>

    <div class="can-editor-form">
      <label>Name<input class="text-input" bind:value={name} oninput={schedulePreview} title={$canHelpBubblesEnabled ? "Display name for this saved CAN signal." : undefined} /></label>
      <label>Signal ID<input class="text-input" bind:value={fieldId} oninput={schedulePreview} disabled={selectedFieldId !== "__new"} title={$canHelpBubblesEnabled ? "Unique CAN signal key; it must start with can: and is fixed while editing an existing field." : undefined} /></label>
      <label>Start bit<input class="text-input" type="number" min="0" max="63" bind:value={startBit} oninput={schedulePreview} title={$canHelpBubblesEnabled ? "First bit position in the payload; numbering is LSB0 within each byte." : undefined} /></label>
      <label>Length<input class="text-input" type="number" min="1" max="64" bind:value={bitLength} oninput={schedulePreview} title={$canHelpBubblesEnabled ? "Number of bits to extract; the selected field must fit in this frame's payload." : undefined} /></label>
      <label>Byte order<select class="text-input" bind:value={byteOrder} onchange={schedulePreview} title={$canHelpBubblesEnabled ? "Intel assembles least-significant bit first; Motorola follows big-endian bit order." : undefined}><option value="intel">Intel</option><option value="motorola">Motorola</option></select></label>
      <label class="can-editor-check" title={$canHelpBubblesEnabled ? "Interpret the extracted value as signed two's-complement; unchecked values are unsigned." : undefined}><input type="checkbox" bind:checked={signed} onchange={schedulePreview} /> Signed</label>
      <label>Factor<input class="text-input" type="number" step="any" bind:value={factor} oninput={schedulePreview} title={$canHelpBubblesEnabled ? "Multiply the raw value by this factor before adding the offset." : undefined} /></label>
      <label>Offset<input class="text-input" type="number" step="any" bind:value={offset} oninput={schedulePreview} title={$canHelpBubblesEnabled ? "Add this amount after multiplying the raw value by the factor." : undefined} /></label>
      <label>Unit<input class="text-input" bind:value={unit} oninput={schedulePreview} title={$canHelpBubblesEnabled ? "Unit label shown with the numeric value; it does not convert that value." : undefined} /></label>
    </div>
    <div class="can-editor-shortcuts" aria-label="Byte-aligned field lengths">
      {#each [8, 16, 32] as length}<button class="button secondary" type="button" onclick={() => alignLength(length)} title={$canHelpBubblesEnabled ? `Set the field to ${length} bits and align its start to this byte (${byteOrder === "motorola" ? "bit 7 for Motorola" : "bit 0 for Intel"}).` : undefined}>{length}-bit</button>{/each}
    </div>

    <div class="can-editor-preview" aria-live="polite">
      <span>Raw <strong>{previewRawValue?.toString() ?? "—"}</strong></span>
      <span>Decoded <strong>{preview ? `${preview.value.toFixed(4)} ${preview.unit}` : "—"}</strong></span>
      {#if preview}<span>Frame {(preview.elapsed_ns / 1e9).toFixed(3)}s</span>{/if}
      {#if definitionError}<span class="can-editor-error">{definitionError}</span>{:else if previewError}<span class="can-editor-error">{previewError}</span>{:else if !preview}<span class="can-muted">Previewing in Rust…</span>{/if}
    </div>
    <footer class="can-editor-actions">
      {#if selectedFieldId !== "__new"}<button class="button danger" type="button" onclick={remove} title={$canHelpBubblesEnabled ? "Remove this saved signal definition; raw CAN capture evidence remains." : undefined}>Remove field</button>{/if}
      <button class="button primary" type="button" onclick={save} disabled={Boolean(definitionError)} title={$canHelpBubblesEnabled ? "Save the signal definition and add its decoded numeric samples to the graph." : undefined}>Save and graph</button>
    </footer>
</dialog>

<style>
  .can-mini-scope { width:100%; height:80px; border:1px solid var(--border); background:var(--surface-deep); border-radius:4px; }
  .can-signal-editor { position:fixed; inset:0; display:grid; gap:8px; width:min(760px, calc(100vw - 24px)); max-width:none; max-height:calc(100vh - 24px); margin:auto; overflow:auto; padding:12px; border:1px solid var(--border-strong); border-radius:9px; color:var(--text); background:var(--surface); box-shadow:0 16px 48px #0009; }
  .can-signal-editor::backdrop { background:#05090dcc; backdrop-filter:blur(2px); }
  .can-editor-heading { display:flex; align-items:center; justify-content:space-between; gap:8px; }
  .can-editor-heading .eyebrow { margin:0 0 2px; font-size:9px; }
  .can-editor-heading h2 { margin:0; font-size:15px; }
  .can-editor-frame, .can-editor-help { margin:0; color:var(--text-muted); font-size:10px; }
  .can-editor-existing, .can-editor-form label { display:grid; gap:3px; color:var(--text-muted); font-size:10px; }
  .can-bit-grid { display:grid; grid-template-columns:repeat(8, minmax(0, 1fr)); gap:4px; min-width:0; }
  .can-bit-byte { display:grid; grid-template-columns:repeat(2, minmax(0, 1fr)); gap:2px; padding:3px; border:1px solid var(--border); border-radius:5px; }
  .can-bit-byte > strong { grid-column:1/-1; color:var(--text-secondary); text-align:center; font:9px ui-monospace, Consolas, monospace; }
  .can-bit-byte button { display:flex; align-items:center; justify-content:space-between; gap:2px; min-width:0; min-height:20px; padding:1px 3px; border:1px solid var(--border); border-radius:3px; color:var(--text-muted); background:var(--surface-deep); font:9px ui-monospace, Consolas, monospace; cursor:pointer; }
  .can-bit-byte button.in-range { border-color:var(--accent); color:var(--text-bright); background:color-mix(in srgb, var(--accent) 24%, var(--surface-deep)); }
  .can-bit-byte button.start-bit { outline:1px solid var(--accent); outline-offset:0; }
  .can-editor-form { display:grid; grid-template-columns:repeat(5, minmax(0, 1fr)); gap:6px; }
  .can-editor-check { align-self:end; display:flex !important; align-items:center; min-height:28px; }
  .can-editor-check input { accent-color:var(--accent); }
  .can-editor-shortcuts, .can-editor-actions { display:flex; align-items:center; flex-wrap:wrap; gap:5px; }
  .can-editor-preview { display:flex; align-items:center; flex-wrap:wrap; gap:6px 16px; padding:6px 8px; border:1px solid var(--border); border-radius:5px; color:var(--text-muted); font-size:10px; }
  .can-editor-preview strong { color:var(--text-bright); font-variant-numeric:tabular-nums; }
  .can-editor-error { color:var(--danger); }
  .can-editor-actions { justify-content:flex-end; }
  @media (max-width:640px) { .can-editor-form { grid-template-columns:repeat(2, minmax(0, 1fr)); } .can-bit-byte { grid-template-columns:1fr; } .can-bit-byte button { justify-content:center; } }
</style>
