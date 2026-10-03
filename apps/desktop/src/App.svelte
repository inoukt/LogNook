<script lang="ts">
  import { onMount } from "svelte";
  import { invoke } from "@tauri-apps/api/core";
  import { listen, type UnlistenFn } from "@tauri-apps/api/event";
  import PlaybackPanel from "./PlaybackPanel.svelte";
  import SignalGraphs from "./SignalGraphs.svelte";
  import type {
    ChartTrace, ConnectionCapabilities, DeviceDescriptor, EngineEvent, Marker, PlaybackState,
    RecordingInfo, Sample, SignalDefinition,
  } from "./types";
  import {
    clampSeekPosition, filterSignals, orderMarkersForTimeline, panTimeWindow,
    swapPositions, zoomTimeWindow, type SignalOption, type TimeWindow,
  } from "./workspace";

  let signalCatalog: SignalDefinition[] = [];
  let sxPorts: string[] = [];
  let selectedSxPort = "";
  let sxIdentity: DeviceDescriptor | undefined;
  let selectedSignals = ["engine_rpm", "coolant_temp", "intake_temp"];
  let latest: Record<string, Sample> = {};
  let series: Record<string, Sample[]> = {};
  let sessionSeries: Record<string, Sample[]> = {};
  let session: RecordingInfo | null = null;
  let playback: PlaybackState = { position_ns: 0, duration_ns: 0, speed: 1, playing: false };
  let timeWindow: TimeWindow = { startNs: 0, endNs: 0 };
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
  let graphRequest = 0;

  $: availableSignals = session
    ? session.signals.map((recorded): SignalOption => {
        const known = signalCatalog.find((signal) => signal.id === recorded.id);
        return known ?? {
          id: recorded.id,
          name: recorded.id,
          unit: recorded.unit,
          description: "Recorded signal",
        };
      })
    : signalCatalog;
  $: visibleSignals = selectedSignals.flatMap((id) => {
    const signal = availableSignals.find((candidate) => candidate.id === id);
    return signal ? [signal] : [];
  });
  $: filteredSignals = filterSignals(availableSignals, signalSearch);
  $: markers = orderMarkersForTimeline(session?.markers ?? []);
  $: traces = visibleSignals.map((signal): ChartTrace => ({
    signalId: signal.id,
    name: signal.name,
    pid: signal.pid,
    unit: signal.unit,
    samples: session ? (sessionSeries[signal.id] ?? []) : (series[signal.id] ?? []),
    current: latest[signal.id],
    sampleRate: sampleRate(signal.id),
  }));
  $: if (session) refreshGraphWindow(session, selectedSignals, timeWindow);

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
      if (sample.origin === "live") {
        if (!session) {
          latest = { ...latest, [sample.signal_id]: sample };
          series = { ...series, [sample.signal_id]: [...(series[sample.signal_id] ?? []), sample].slice(-180) };
        }
      } else {
        latest = { ...latest, [sample.signal_id]: sample };
      }
    } else if (event.type === "marker") {
      if (session && !session.markers.some((marker) => marker.id === event.payload.id)) {
        session = { ...session, markers: [...session.markers, event.payload] };
      }
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
      replaying = true;
      status = "Replaying session";
    } else if (event.type === "playbackState") {
      playback = event.payload;
      replaying = playback.playing;
      status = playback.playing ? "Replaying session" : "Session paused";
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

  async function clearSession() {
    session = null;
    sessionSeries = {};
    playback = { position_ns: 0, duration_ns: 0, speed: 1, playing: false };
  }

  async function activateSession(info: RecordingInfo) {
    session = info;
    selectedSignals = info.signals.map((signal) => signal.id);
    timeWindow = { startNs: 0, endNs: info.duration_ns };
    latest = {};
    series = {};
    sessionSeries = {};
    playback = await invoke<PlaybackState>("playback_state");
    recordingPath = "loaded";
    exportPath = "";
    replaying = false;
    if (!connected) status = "Session loaded";
    signalSearch = "";
  }

  async function refreshGraphWindow(info: RecordingInfo, signalIds: string[], range: TimeWindow) {
    const request = ++graphRequest;
    if (!signalIds.length) { sessionSeries = {}; return; }
    try {
      const samples = await invoke<Sample[]>("recording_window", {
        signalIds,
        startNs: range.startNs,
        endNs: range.endNs,
        maxBuckets: 1_000,
      });
      if (request !== graphRequest || info !== session) return;
      const grouped: Record<string, Sample[]> = {};
      for (const sample of samples) (grouped[sample.signal_id] ??= []).push(sample);
      sessionSeries = grouped;
    } catch (reason) {
      if (request === graphRequest) showError(reason);
    }
  }

  function connect() {
    return run(async () => {
      const requested = selectedSignals.filter((id) => signalCatalog.some((signal) => signal.id === id));
      if (!requested.length) throw new Error("Select at least one supported engine signal first");
      const capabilities = await invoke<ConnectionCapabilities>("connect_simulator", { signalIds: requested });
      await clearSession();
      series = {};
      latest = {};
      selectedSignals = selectedSignals.filter((id) => capabilities.supported_signals.some((signal) => signal.id === id));
    });
  }

  function connectSx() {
    return run(async () => {
      const requested = selectedSignals.filter((id) => signalCatalog.some((signal) => signal.id === id));
      if (!requested.length) throw new Error("Select at least one supported engine signal first");
      const capabilities = await invoke<ConnectionCapabilities>("connect_obdlink_sx", {
        portName: selectedSxPort,
        signalIds: requested,
      });
      await clearSession();
      series = {};
      latest = {};
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
      await clearSession();
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
      await activateSession(await invoke<RecordingInfo>("load_last_recording"));
    });
  }

  function insertLiveMarker() {
    return run(async () => {
      await invoke("insert_marker", { label: "Manual marker", note: markerNote.trim() });
      markerNote = "";
    });
  }

  function exportCsv() {
    return run(async () => { exportPath = await invoke<string>("export_csv"); });
  }

  function openCsv() {
    return run(async () => {
      const info = await invoke<RecordingInfo | null>("open_csv");
      if (info) await activateSession(info);
    });
  }

  function togglePlayback() {
    return run(async () => {
      if (playback.playing) await invoke("pause_replay");
      else await invoke("start_replay", { speed: playback.speed });
      playback = await invoke<PlaybackState>("playback_state");
    });
  }

  function returnToStart() {
    return run(async () => {
      await invoke("seek_replay", { positionNs: 0 });
      playback = await invoke<PlaybackState>("playback_state");
    });
  }

  function seek(positionNs: number) {
    return run(async () => {
      await invoke("seek_replay", { positionNs: clampSeekPosition(positionNs, playback.duration_ns) });
      playback = await invoke<PlaybackState>("playback_state");
    });
  }

  function setSpeed(speed: number) {
    return run(async () => {
      await invoke("set_replay_speed", { speed });
      playback = await invoke<PlaybackState>("playback_state");
    });
  }

  function addMarker(label: string, note: string) {
    return run(async () => {
      const marker = await invoke<Marker>("add_playback_marker", { label, note });
      if (session) session = { ...session, markers: [...session.markers, marker] };
    });
  }

  function updateMarker(id: number, label: string, note: string) {
    return run(async () => {
      const marker = await invoke<Marker>("update_playback_marker", { id, label, note });
      if (session) session = { ...session, markers: session.markers.map((item) => item.id === id ? marker : item) };
    });
  }

  function deleteMarker(id: number) {
    return run(async () => {
      await invoke("delete_playback_marker", { id });
      if (session) session = { ...session, markers: session.markers.filter((marker) => marker.id !== id) };
      playback = await invoke<PlaybackState>("playback_state");
    });
  }

  function zoom(factor: number) {
    if (!session) return;
    timeWindow = zoomTimeWindow(timeWindow, playback.position_ns, factor, playback.duration_ns);
  }

  function pan(fraction: number) {
    if (!session) return;
    timeWindow = panTimeWindow(
      timeWindow,
      (timeWindow.endNs - timeWindow.startNs) * fraction,
      playback.duration_ns,
    );
  }

  function resetZoom() {
    if (session) timeWindow = { startNs: 0, endNs: playback.duration_ns };
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
    const points = session ? sessionSeries[signalId] ?? [] : series[signalId] ?? [];
    if (points.length < 2) return "— Hz";
    const previous = points.at(-2)!;
    const current = points.at(-1)!;
    const delta = current.elapsed_ns - previous.elapsed_ns;
    return delta > 0 ? `${(1e9 / delta).toFixed(1)} Hz` : "— Hz";
  }
</script>

<svelte:head>
  <title>LogNook · Acquisition</title>
  <meta name="description" content="Read-only OBD acquisition, recording, and CSV playback." />
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
            <button class="button secondary" onclick={insertLiveMarker} disabled={busy}>＋ Marker</button>
          {:else}
            <button class="button secondary" onclick={exportCsv} disabled={busy || (!recordingPath && !session)}>Export CSV</button>
          {/if}
        </div>
      </div>
      <div class="connection-actions">
        {#if connected}
          <button class="button secondary" onclick={disconnect} disabled={busy}>Disconnect</button>
        {:else}
          <select class="text-input port-select" bind:value={selectedSxPort} onchange={() => sxIdentity = undefined} aria-label="OBDLink SX serial port" disabled={busy || sxPorts.length === 0 || replaying}>
            {#if sxPorts.length === 0}<option value="">No SX found</option>{/if}
            {#each sxPorts as port}<option value={port}>{port}</option>{/each}
          </select>
          <button class="button secondary" onclick={identifySx} disabled={busy || !selectedSxPort || replaying}>Identify SX</button>
          <button class="button secondary" onclick={connectSx} disabled={busy || !selectedSxPort || sxIdentity?.path !== selectedSxPort || selectedSignals.length === 0 || replaying}>Connect OBDLink SX</button>
          <button class="button primary" onclick={connect} disabled={busy || selectedSignals.length === 0 || replaying}>Connect simulator</button>
        {/if}
        {#if recording}
          <button class="button danger" onclick={stopRecording} disabled={busy}>Stop recording</button>
        {:else}
          <button class="button record" onclick={startRecording} disabled={busy || !connected || replaying}>● Record</button>
        {/if}
      </div>
    </section>

    {#if error}<div class="notice error" role="alert">{error}</div>{/if}
    {#if sxIdentity}<div class="notice success">Identified {sxIdentity.hardware_family} · {sxIdentity.firmware} · serial {sxIdentity.stable_id}. Connect the vehicle before live acquisition.</div>{/if}
    {#if recording}<div class="notice recording"><span class="pulse-dot"></span> Recording live samples · {recordingPath}</div>{/if}
    {#if exportPath}<div class="notice success">CSV exported · {exportPath}</div>{/if}

    <PlaybackPanel
      info={session}
      {playback}
      {markers}
      range={timeWindow}
      openDisabled={connected}
      disabled={busy || recording}
      onOpen={openCsv}
      onTogglePlay={togglePlayback}
      onStart={returnToStart}
      onSeek={seek}
      onSpeed={setSpeed}
      onAddMarker={addMarker}
      onUpdateMarker={updateMarker}
      onDeleteMarker={deleteMarker}
      onJumpMarker={seek}
      onZoom={zoom}
      onPan={pan}
      onReset={resetZoom}
    />

    <div class="workspace-grid">
      <aside class="panel signal-panel">
        <div class="panel-heading"><div><p class="eyebrow">SIGNAL PICKER</p><h2>{session ? "Recorded signals" : "Engine · Mode 01"}</h2></div><span class="count-pill">{selectedSignals.length} selected</span></div>
        <input class="text-input signal-search" type="search" bind:value={signalSearch} placeholder="Search PID, name or unit" aria-label="Search signals" />
        <div class="signal-list">
          {#each filteredSignals as signal (signal.id)}
            <label class="signal-option">
              <input type="checkbox" checked={selectedSignals.includes(signal.id)} onchange={() => toggleSignal(signal.id)} disabled={connected || recording} />
              <span class="signal-copy"><span>{signal.name}</span><small>{signal.pid === undefined ? signal.id : `PID ${signal.pid.toString(16).toUpperCase().padStart(2, "0")}`} · {signal.unit}</small></span>
              <span class="checkmark" aria-hidden="true"></span>
            </label>
          {:else}
            <p class="no-signals">{availableSignals.length ? "No matching signals" : "Loading signals…"}</p>
          {/each}
        </div>
        <p class="panel-hint">{session ? "Session signals are stored as recorded. The list scrolls independently." : "Read-only engine signals. The list scrolls independently when it grows."}</p>
      </aside>

      <section class="measurements" aria-label="Measurements">
        <div class="measurement-heading"><div><p class="eyebrow">MEASUREMENTS</p><h2>{session ? "Recorded data" : "Live values"}</h2></div><span class="rate-note">{graphMode === "overlay" ? "Combined signals · shared time" : "Drag to swap · resize at corner"}</span></div>
        {#if visibleSignals.length === 0}
          <div class="empty-state"><span class="empty-icon">⌁</span><h3>{session ? "No signals selected" : "Select signals to begin"}</h3><p>{session ? "Choose recorded signals to add them to the graph." : "Choose one or more PIDs, then connect an adapter."}</p></div>
        {:else}
          <SignalGraphs
            {traces}
            {graphMode}
            onReorder={swapGraphPositions}
            timeWindow={session ? timeWindow : undefined}
            markers={session ? markers : []}
            playheadNs={session ? playback.position_ns : undefined}
          />
        {/if}
      </section>
    </div>
  </main>
</div>
