<script lang="ts">
  import { canHelpBubblesEnabled, setCanHelpBubblesEnabled } from "./canHelp";

  let dialog: HTMLDialogElement;
  let trigger: HTMLButtonElement | undefined;

  function changeBubbleSetting(event: Event) {
    setCanHelpBubblesEnabled((event.currentTarget as HTMLInputElement).checked);
  }

  function closeHelp() {
    dialog.close();
  }
</script>

<button class="can-help-trigger" type="button" bind:this={trigger} aria-label="Open CAN help" onclick={() => dialog.showModal()}>?</button>

<dialog class="can-help-dialog" bind:this={dialog} aria-labelledby="can-help-title" onclose={() => trigger?.focus()}>
  <header class="can-help-heading">
    <div><p class="eyebrow">TEST WORKBENCH</p><h2 id="can-help-title">CAN bus help</h2></div>
    <button class="button secondary" type="button" onclick={closeHelp}>Close</button>
  </header>

  <label class="can-help-toggle"><input type="checkbox" checked={$canHelpBubblesEnabled} onchange={changeBubbleSetting} /> Show control tips on hover</label>

  <div class="can-help-content">
    <section>
      <h3>Connect and choose a mode</h3>
      <p>Select the GS_USB/Candlelight adapter, interface, channel, and bitrate that match the CAN bus. Listen-only receives frames without transmitting; change this setting by disconnecting and reconnecting.</p>
      <p>Scans and manual frame sends require an active connection and a paired capture. Connecting or opening Help does not send CAN frames.</p>
    </section>

    <section>
      <h3>Watch traffic and mark events</h3>
      <p>The monitor shows per-ID counts and RX rates, recent frames (up to 400), and event candidates. Start a capture to save raw traffic and evidence as CSV plus JSONL.</p>
      <p>Enter a label and before/after window, then mark each physical action during capture. For example, mark six traction-control toggles with the same label in one 30-second capture. Repeated marks rank IDs and byte changes around each event; these are clues to inspect, not decoded meanings.</p>
      <p>Scope a candidate byte or bit to open the field editor. Follow Live RX updates the byte grid and decoded preview; turn it off to inspect the original event frame. The scope shows recent matching RX frames independently of Follow Live RX. Counter-like and checksum-like noise hints help prioritize inspection.</p>
      <p>For a bounded trigger capture, set pre- and post-trigger times and a maximum frame count, arm the window, mark the trigger, wait for post-trigger collection to finish, then save the raw window as CSV plus JSONL.</p>
    </section>

    <section>
      <h3>Scan, define, or send</h3>
      <p>Choose a CAN-ID range, Mode 01 support/PIDs, Mode 21 one-byte identifiers (<code>00–FF</code>), or Mode 22 two-byte DIDs (<code>0000–FFFF</code>). Set the ECU request/response route, range, pacing, and reply timeout; Stop cancels remaining requests. “Try CAN once” sends one Mode 01 PID 00 request on that route.</p>
      <p>Add a field for an ID, then set its start bit, length, byte order, signedness, factor, offset, and unit. The field needs a CAN connection; its decoded samples use the shared graph and recording pipeline. Manual frame controls send the configured ID and bytes at the chosen pacing and repeat count, with TX/RX kept in the paired capture.</p>
    </section>

    <section>
      <h3>Replay and use decoded samples</h3>
      <p>Use Save &amp; Graph in the field editor to publish a decoded signal through the shared measurement graph, recording, and CSV flow. Top Record stores selected decoded CAN channels in a normal LogNook session; raw CAN Capture is separate CSV plus JSONL evidence. A raw frame becomes a signal only after you define and save a field.</p>
      <p>Open a saved <code>.candump.csv</code> capture to review frames, rates, event correlations, and JSONL evidence. Compare another capture or export an explicitly unverified DBC skeleton. The active capture path appears in Capture; replay and comparison paths appear in the report.</p>
    </section>
  </div>
</dialog>

<style>
  .can-help-trigger { width:24px; height:24px; flex:0 0 24px; padding:0; border:1px solid var(--border-strong); border-radius:50%; color:var(--text-secondary); background:var(--surface-raised); cursor:pointer; font-size:12px; font-weight:700; line-height:1; }
  .can-help-trigger:hover { color:var(--accent); border-color:var(--accent); }
  .can-help-trigger:focus-visible,.can-help-dialog button:focus-visible { outline:2px solid var(--accent); outline-offset:2px; }
  .can-help-dialog { box-sizing:border-box; width:min(540px, calc(100vw - 24px)); max-width:calc(100vw - 24px); max-height:calc(100vh - 24px); max-height:calc(100dvh - 24px); overflow:auto; padding:16px; border:1px solid var(--border); border-radius:10px; color:var(--text); background:var(--surface); box-shadow:0 16px 48px #0009; }
  .can-help-dialog::backdrop { background:#05090dcc; backdrop-filter:blur(2px); }
  .can-help-heading { display:flex; align-items:center; justify-content:space-between; gap:12px; margin-bottom:12px; }
  .can-help-heading .eyebrow { margin-bottom:3px; }
  .can-help-heading h2 { font-size:16px; }
  .can-help-toggle { display:flex; align-items:center; gap:7px; margin:0 0 12px; color:var(--text-secondary); font-size:12px; cursor:pointer; }
  .can-help-toggle input { accent-color:var(--accent); }
  .can-help-content { display:grid; gap:11px; }
  .can-help-content section { padding-bottom:9px; border-bottom:1px solid var(--border); }
  .can-help-content section:last-child { padding-bottom:0; border-bottom:0; }
  .can-help-content h3 { margin:0 0 4px; color:var(--text-bright); font-size:12px; }
  .can-help-content p { margin:0 0 5px; color:var(--text-secondary); font-size:11px; line-height:1.45; }
  .can-help-content p:last-child { margin-bottom:0; }
  .can-help-content code { color:var(--text-bright); }
</style>
