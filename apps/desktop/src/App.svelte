<script lang="ts">
  import { onMount } from "svelte";
  import { invoke } from "@tauri-apps/api/core";
  import { listen, type UnlistenFn } from "@tauri-apps/api/event";
  import SignalGraphs from "./SignalGraphs.svelte";
  import type { ChartTrace, ConnectionCapabilities, DeviceDescriptor, EngineEvent, Sample, SignalDefinition } from "./types";
  import { filterSignals, swapPositions } from "./workspace";

  let signalCatalog: SignalDefinition[] = [];
  let sxPorts: string[] = [];
  let selectedSxPort = "";
  let sxIdentity: DeviceDescriptor | undefined;
  let selectedSignals = ["engine_rpm", "coolant_temp", "intake_temp"];
  let latest: Record<string, Sample> = {};
  let series: Record<string, Sample[]> = {};
  let connected = false;
  let recording = false;
  let replaying = false;
  let busy = false;
  let status = "Disconnected";
  let recordingPath = "";
  let exportPath = "";
  let markerNote = "";
  let error = "";
  let signalSearch = "";
  let graphMode: "overlay" | "separate" = "overlay";
  let unlisten: UnlistenFn | undefined;

  $: visibleSignals = selectedSignals.flatMap((id) => {
    const signal = signalCatalog.find((candidate) => candidate.id === id);
    return signal ? [signal] : [];
  });
  $: filteredSignals = filterSignals(signalCatalog, signalSearch);
  $: traces = visibleSignals.map((signal): ChartTrace => ({
    signalId: signal.id,
    name: signal.name,
    pid: signal.pid,
    unit: signal.unit,
    samples: series[signal.id] ?? [],
    current: latest[signal.id],
    sampleRate: sampleRate(signal.id),
  }));

  onMount(() => {
    let disposed = false;
    listen<EngineEvent>("lognook-event", ({ payload }) => handleEvent(payload))
      .then((stop) => disposed ? stop() : unlisten = stop)
      .catch(showError);
    invoke<SignalDefinition[]>("available_signals")
      .then((signals) => signalCatalog = signals)
      .catch(() => error = "Open LogNook as a desktop app to access the acquisition engine.");
    invoke<string[]>("available_obdlink_sx_ports")
      .then((ports) => {
        sxPorts = ports;
        selectedSxPort = ports[0] ?? "";
      })
      .catch(() => sxPorts = []);
    return () => { disposed = true; unlisten?.(); };
  });

  function handleEvent(event: EngineEvent) {
    if (event.type === "sample") {
      const sample = event.payload;
      latest = { ...latest, [sample.signal_id]: sample };
      series = { ...series, [sample.signal_id]: [...(series[sample.signal_id] ?? []), sample].slice(-180) };
    } else if (event.type === "connected") {
      connected = true;
      status = event.payload.device.hardware_family === "simulator"
        ? "Simulated OBD · read only"
        : `${event.payload.device.hardware_family} · read only`;
    } else if (event.type === "disconnected") {
      connected = false;
      status = "Disconnected";
    } else if (event.type === "playbackStarted") {
      latest = {};
      series = {};
      replaying = true;
      status = "Replaying recording";
    } else if (event.type === "playbackFinished") {
      replaying = false;
      status = "Replay complete";
    } else if (event.type === "recordingFailed") {
      recording = false;
      recordingPath = event.payload.path;
      status = `Recording stopped · preserved ${event.payload.sample_count} samples`;
    } else if (event.type === "error") {
      showError(event.payload);
    }
  }

  async function run(action: () => Promise<void>) {
    error = "";
    busy = true;
    try { await action(); } catch (reason) { showError(reason); }
    finally { busy = false; }
  }

  function showError(reason: unknown) {
    error = reason instanceof Error ? reason.message : String(reason);
  }

  function connect() {
    return run(async () => {
      series = {};
      latest = {};
      const capabilities = await invoke<ConnectionCapabilities>(
        "connect_simulator",
        { signalIds: selectedSignals },
      );
      selectedSignals = selectedSignals.filter((id) => capabilities.supported_signals.some((signal) => signal.id === id));
    });
  }

  function connectSx() {
    return run(async () => {
      series = {};
      latest = {};
      const capabilities = await invoke<ConnectionCapabilities>(
        "connect_obdlink_sx",
        { portName: selectedSxPort, signalIds: selectedSignals },
      );
      selectedSignals = selectedSignals.filter((id) => capabilities.supported_signals.some((signal) => signal.id === id));
    });
  }

  function identifySx() {
    return run(async () => {
      sxIdentity = undefined;
      sxIdentity = await invoke<DeviceDescriptor>("identify_obdlink_sx", { portName: selectedSxPort });
    });
  }

  function disconnect() {
    return run(async () => { await invoke("disconnect"); connected = false; recording = false; });
  }

  function startRecording() {
    return run(async () => {
      recordingPath = await invoke<string>("start_recording");
      recording = true;
      exportPath = "";
    });
  }

  function stopRecording() {
    return run(async () => {
      const result = await invoke<{ path: string; sample_count: number }>("stop_recording");
      recordingPath = result.path;
      recording = false;
      status = `Saved ${result.sample_count.toLocaleString()} samples`;
    });
  }

  function insertMarker() {
    return run(async () => {
      await invoke("insert_marker", { label: "Manual marker", note: markerNote.trim() });
      markerNote = "";
    });
  }

  function exportCsv() {
    return run(async () => { exportPath = await invoke<string>("export_csv"); });
  }

  function replay() {
    return run(async () => { await invoke("replay_last"); });
  }

  function stopReplay() {
    return run(async () => { await invoke("stop_replay"); replaying = false; });
  }

  function toggleSignal(signalId: string) {
    selectedSignals = selectedSignals.includes(signalId)
      ? selectedSignals.filter((id) => id !== signalId)
      : [...selectedSignals, signalId];
  }

  function swapGraphPositions(fromId: string, toId: string) {
    selectedSignals = swapPositions(selectedSignals, selectedSignals.indexOf(fromId), selectedSignals.indexOf(toId));
  }

  function sampleRate(signalId: string): string {
    const points = series[signalId] ?? [];
    if (points.length < 2) return "— Hz";
    const previous = points.at(-2)!;
    const current = points.at(-1)!;
    const delta = current.elapsed_ns - previous.elapsed_ns;
    return delta > 0 ? `${(1e9 / delta).toFixed(1)} Hz` : "— Hz";
  }
</script>

<svelte:head>
  <title>LogNook · Live</title>
  <meta name="description" content="Read-only OBD acquisition and recording." />
</svelte:head>

<div class="app-shell">
  <header class="topbar">
    <a class="brand" href="/" aria-label="LogNook home"><span class="brand-mark">L</span><span>Log<span class="brand-accent">Nook</span></span></a>
    <div class="connection-state"><span class:online={connected} class:replay={replaying} class="state-dot"></span><span>{status}</span></div>
    <span class="milestone">FOUNDATION POC</span>
  </header>

  <main>
    <section class="workspace-toolbar" aria-label="Acquisition controls">
      <div class="workspace-title"><p class="eyebrow">LIVE WORKSPACE</p><h1>Acquisition</h1></div>
      <div class="toolbar-center">
        <div class="mode-control">
          <span class="toolbar-label">GRAPH VIEW</span>
          <div class="mode-switch" role="group" aria-label="Graph view">
            <button class:active={graphMode === "overlay"} aria-pressed={graphMode === "overlay"} onclick={() => graphMode = "overlay"}>Combined</button>
            <button class:active={graphMode === "separate"} aria-pressed={graphMode === "separate"} onclick={() => graphMode = "separate"}>Separate</button>
          </div>
        </div>
        <div class="session-actions">
          {#if recording}
            <input class="text-input toolbar-input" bind:value={markerNote} placeholder="Marker note (optional)" aria-label="Marker note" disabled={busy} />
            <button class="button secondary" onclick={insertMarker} disabled={busy}>＋ Marker</button>
          {:else}
            <button class="button secondary" onclick={exportCsv} disabled={busy || !recordingPath}>Export CSV</button>
            {#if replaying}
              <button class="button secondary" onclick={stopReplay} disabled={busy}>Stop replay</button>
            {:else}
              <button class="button secondary" onclick={replay} disabled={busy || !recordingPath}>Replay</button>
            {/if}
          {/if}
        </div>
      </div>
      <div class="connection-actions">
        {#if connected}
          <button class="button secondary" onclick={disconnect} disabled={busy}>Disconnect</button>
        {:else}
          <select class="text-input port-select" bind:value={selectedSxPort} onchange={() => sxIdentity = undefined} aria-label="OBDLink SX serial port" disabled={busy || sxPorts.length === 0}>
            {#if sxPorts.length === 0}<option value="">No SX found</option>{/if}
            {#each sxPorts as port}<option value={port}>{port}</option>{/each}
          </select>
          <button class="button secondary" onclick={identifySx} disabled={busy || !selectedSxPort}>Identify SX</button>
          <button class="button secondary" onclick={connectSx} disabled={busy || !selectedSxPort || sxIdentity?.path !== selectedSxPort || selectedSignals.length === 0}>Connect OBDLink SX</button>
          <button class="button primary" onclick={connect} disabled={busy || selectedSignals.length === 0}>Connect simulator</button>
        {/if}
        {#if recording}
          <button class="button danger" onclick={stopRecording} disabled={busy}>Stop recording</button>
        {:else}
          <button class="button record" onclick={startRecording} disabled={busy || !connected}>● Record</button>
        {/if}
      </div>
    </section>

    {#if error}<div class="notice error" role="alert">{error}</div>{/if}
    {#if sxIdentity}<div class="notice success">Identified {sxIdentity.hardware_family} · {sxIdentity.firmware} · serial {sxIdentity.stable_id}. Connect the vehicle before live acquisition.</div>{/if}
    {#if recording}<div class="notice recording"><span class="pulse-dot"></span> Recording live samples · {recordingPath}</div>{/if}
    {#if exportPath}<div class="notice success">CSV exported · {exportPath}</div>{/if}

    <div class="workspace-grid">
      <aside class="panel signal-panel">
        <div class="panel-heading"><div><p class="eyebrow">SIGNAL PICKER</p><h2>Engine · Mode 01</h2></div><span class="count-pill">{selectedSignals.length} selected</span></div>
        <input class="text-input signal-search" type="search" bind:value={signalSearch} placeholder="Search PID, name or unit" aria-label="Search signals" />
        <div class="signal-list">
          {#each filteredSignals as signal (signal.id)}
            <label class="signal-option">
              <input type="checkbox" checked={selectedSignals.includes(signal.id)} onchange={() => toggleSignal(signal.id)} disabled={connected} />
              <span class="signal-copy"><span>{signal.name}</span><small>PID {signal.pid.toString(16).toUpperCase().padStart(2, "0")} · {signal.unit}</small></span>
              <span class="checkmark" aria-hidden="true"></span>
            </label>
          {:else}
            <p class="no-signals">{signalCatalog.length ? "No matching signals" : "Loading signals…"}</p>
          {/each}
        </div>
        <p class="panel-hint">Read-only engine signals. The list scrolls independently when it grows.</p>
      </aside>

      <section class="measurements" aria-label="Live measurements">
        <div class="measurement-heading"><div><p class="eyebrow">MEASUREMENTS</p><h2>{replaying ? "Recorded data" : "Live values"}</h2></div><span class="rate-note">{graphMode === "overlay" ? "Combined signals · shared time" : "Drag to swap · resize at corner"}</span></div>
        {#if visibleSignals.length === 0}
          <div class="empty-state"><span class="empty-icon">⌁</span><h3>Select signals to begin</h3><p>Choose one or more PIDs, then connect an adapter.</p></div>
        {:else}
          <SignalGraphs {traces} {graphMode} onReorder={swapGraphPositions} />
        {/if}
      </section>
    </div>
  </main>
</div>
