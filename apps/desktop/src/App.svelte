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
    averageInWindow, clampSeekPosition, filterSignals, followPlayhead, orderMarkersForTimeline,
    panTimeWindow, sampleTimeRange, scrubTimeWindow, setGraphSignal, setLogSignal, signalColor, swapPositions,
    zoomTimeWindow, type SignalOption, type TimeWindow,
  } from "./workspace";

  let signalCatalog: SignalDefinition[] = [];
  let sxPorts: string[] = [];
  let selectedSxPort = "";
  let sxIdentity: DeviceDescriptor | undefined;
  let logSignalIds = ["engine_rpm", "coolant_temp", "intake_temp"];
  let graphSignalIds = [...logSignalIds];
  let latest: Record<string, Sample> = {};
  let series: Record<string, Sample[]> = {};
  let sessionSeries: Record<string, Sample[]> = {};
  let sessionAverages: Record<string, number> = {};
  let signalColors: Record<string, string> = {};
  let session: RecordingInfo | null = null;
  let liveGraphSignalIds: string[] | null = null;
  let playback: PlaybackState = { session_id: 0, position_ns: 0, duration_ns: 0, speed: 1, playing: false };
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
  let signalPickerVisible = true;
  let wheelScrubbing = false;
  let statusHeight = 42;
  let statusResize: { y: number; height: number } | null = null;
  let graphMode: "overlay" | "separate" = "overlay";
  let unlisten: UnlistenFn | undefined;
  let graphRequest = 0;
  let readoutRequest = 0;
  let graphRefreshTimer: number | undefined;
  let pendingGraphRefresh: { info: RecordingInfo; signalIds: string[]; range: TimeWindow } | undefined;
  let lastGraphRefreshAt = 0;
  let scrubPending: number | null = null;
  let scrubInFlight: number | null = null;

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
  $: visibleSignals = graphSignalIds.flatMap((id) => {
    const signal = availableSignals.find((candidate) => candidate.id === id);
    return signal ? [signal] : [];
  });
  $: filteredSignals = filterSignals(availableSignals, signalSearch);
  $: liveGraphRange = sampleTimeRange(visibleSignals.flatMap((signal) => series[signal.id] ?? []));
  $: signalAverages = session
    ? sessionAverages
    : Object.fromEntries(availableSignals.map((signal) => [
        signal.id,
        averageInWindow(series[signal.id] ?? [], liveGraphRange),
      ]));
  $: markers = orderMarkersForTimeline(session?.markers ?? []);
  $: traces = visibleSignals.map((signal): ChartTrace => ({
    signalId: signal.id,
    name: signal.name,
    pid: signal.pid,
    unit: signal.unit,
    color: signalColor(signal.id, availableSignals.indexOf(signal), signalColors),
    samples: session ? (sessionSeries[signal.id] ?? []) : (series[signal.id] ?? []),
    current: latest[signal.id],
    sampleRate: sampleRate(signal.id),
  }));
  $: if (session) scheduleGraphWindow(session, graphSignalIds, timeWindow);

  onMount(() => {
    let disposed = false;
    try {
      const colors = JSON.parse(localStorage.getItem("lognook.signal-colors") ?? "{}");
      if (colors && typeof colors === "object" && !Array.isArray(colors)) signalColors = colors;
      wheelScrubbing = localStorage.getItem("lognook.wheel-scrubbing") === "true";
    } catch { /* Ignore unavailable or malformed local preferences. */ }
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
    if (event.type === "playbackSample") {
      if (event.payload.session_id !== playback.session_id || scrubPending !== null || scrubInFlight !== null) return;
      const sample = event.payload.sample;
      readoutRequest++;
      latest = { ...latest, [sample.signal_id]: sample };
    } else if (event.type === "sample") {
      const sample = event.payload;
      if (sample.origin === "live") {
        if (!session) {
          latest = { ...latest, [sample.signal_id]: sample };
          series = { ...series, [sample.signal_id]: [...(series[sample.signal_id] ?? []), sample].slice(-180) };
        }
      }
    } else if (event.type === "playbackMarker") {
      if (event.payload.session_id !== playback.session_id) return;
      const marker = event.payload.marker;
      if (session && !session.markers.some((item) => item.id === marker.id)) {
        session = { ...session, markers: [...session.markers, marker] };
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
    } else if (event.type === "playbackState") {
      if (event.payload.session_id !== playback.session_id) return;
      const pendingPosition = scrubPending ?? scrubInFlight;
      playback = pendingPosition === null ? event.payload : { ...event.payload, position_ns: pendingPosition };
      if (playback.playing && session) {
        let followRange = timeWindow;
        if (followRange.endNs - followRange.startNs >= playback.duration_ns) {
          followRange = { startNs: 0, endNs: 30_000_000_000 };
        }
        timeWindow = followPlayhead(followRange, playback.position_ns, playback.duration_ns);
      }
      replaying = playback.playing;
      status = playback.playing
        ? "Replaying session"
        : playback.duration_ns > 0 && playback.position_ns >= playback.duration_ns
          ? "Replay complete"
          : "Session paused";
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
    readoutRequest++;
    graphRequest++;
    if (graphRefreshTimer !== undefined) clearTimeout(graphRefreshTimer);
    graphRefreshTimer = undefined;
    pendingGraphRefresh = undefined;
    scrubPending = null;
    if (liveGraphSignalIds) graphSignalIds = liveGraphSignalIds;
    liveGraphSignalIds = null;
    session = null;
    latest = {};
    sessionSeries = {};
    sessionAverages = {};
    replaying = false;
    playback = { session_id: 0, position_ns: 0, duration_ns: 0, speed: 1, playing: false };
    signalSearch = "";
    if (!connected) status = "Disconnected";
  }

  async function activateSession(info: RecordingInfo) {
    readoutRequest++;
    graphRequest++;
    scrubPending = null;
    if (!session) liveGraphSignalIds = [...graphSignalIds];
    session = info;
    playback = { session_id: 0, position_ns: 0, duration_ns: info.duration_ns, speed: 1, playing: false };
    graphSignalIds = info.signals.map((signal) => signal.id);
    timeWindow = { startNs: 0, endNs: info.duration_ns };
    latest = {};
    series = {};
    sessionSeries = {};
    sessionAverages = {};
    playback = await invoke<PlaybackState>("playback_state");
    await refreshPlaybackReadouts(playback.position_ns, info.signals.map((signal) => signal.id));
    recordingPath = "loaded";
    exportPath = "";
    replaying = false;
    if (!connected) status = "Session loaded";
    signalSearch = "";
  }

  async function refreshGraphWindow(info: RecordingInfo, signalIds: string[], range: TimeWindow) {
    const request = ++graphRequest;
    try {
      const [samples, averages] = await Promise.all([
        signalIds.length
          ? invoke<Sample[]>("recording_window", {
              signalIds,
              startNs: range.startNs,
              endNs: range.endNs,
              maxBuckets: 360,
            })
          : Promise.resolve([]),
        invoke<Record<string, number>>("recording_averages", { startNs: range.startNs, endNs: range.endNs }),
      ]);
      if (request !== graphRequest || info !== session) return;
      const grouped: Record<string, Sample[]> = {};
      for (const sample of samples) (grouped[sample.signal_id] ??= []).push(sample);
      sessionSeries = grouped;
      sessionAverages = averages;
    } catch (reason) {
      if (request === graphRequest) showError(reason);
    }
  }

  async function refreshPlaybackReadouts(positionNs: number, signalIds: string[]) {
    const request = ++readoutRequest;
    const values = await invoke<Sample[]>("recording_values_at", { signalIds, positionNs });
    if (request === readoutRequest && session) {
      latest = Object.fromEntries(values.map((sample) => [sample.signal_id, sample]));
    }
  }

  function connect() {
    return run(async () => {
      const requested = logSignalIds.filter((id) => signalCatalog.some((signal) => signal.id === id));
      if (!requested.length) throw new Error("Select at least one supported engine signal first");
      const capabilities = await invoke<ConnectionCapabilities>("connect_simulator", { signalIds: requested });
      await clearSession();
      series = {};
      latest = {};
      logSignalIds = logSignalIds.filter((id) => capabilities.supported_signals.some((signal) => signal.id === id));
      graphSignalIds = graphSignalIds.filter((id) => capabilities.supported_signals.some((signal) => signal.id === id));
    });
  }

  function connectSx() {
    return run(async () => {
      const requested = logSignalIds.filter((id) => signalCatalog.some((signal) => signal.id === id));
      if (!requested.length) throw new Error("Select at least one supported engine signal first");
      const capabilities = await invoke<ConnectionCapabilities>("connect_obdlink_sx", {
        portName: selectedSxPort,
        signalIds: requested,
      });
      await clearSession();
      series = {};
      latest = {};
      logSignalIds = logSignalIds.filter((id) => capabilities.supported_signals.some((signal) => signal.id === id));
      graphSignalIds = graphSignalIds.filter((id) => capabilities.supported_signals.some((signal) => signal.id === id));
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

  function closeSession() {
    return run(async () => {
      await invoke("stop_replay");
      await clearSession();
    });
  }

  function togglePlayback() {
    return run(async () => {
      if (playback.playing) await invoke("pause_replay");
      else {
        if (session && playback.duration_ns > 0) {
          const currentSpan = timeWindow.endNs - timeWindow.startNs;
          const view = currentSpan >= playback.duration_ns
            ? { startNs: 0, endNs: 30_000_000_000 }
            : timeWindow;
          timeWindow = followPlayhead(view, playback.position_ns, playback.duration_ns);
        }
        await invoke("start_replay", { speed: playback.speed });
      }
      playback = await invoke<PlaybackState>("playback_state");
    });
  }

  function scheduleGraphWindow(info: RecordingInfo, signalIds: string[], range: TimeWindow) {
    pendingGraphRefresh = { info, signalIds: [...signalIds], range: { ...range } };
    if (graphRefreshTimer !== undefined) return;
    const delay = Math.max(0, 100 - (performance.now() - lastGraphRefreshAt));
    graphRefreshTimer = window.setTimeout(() => {
      graphRefreshTimer = undefined;
      lastGraphRefreshAt = performance.now();
      const request = pendingGraphRefresh;
      pendingGraphRefresh = undefined;
      if (request && request.info === session) {
        refreshGraphWindow(request.info, request.signalIds, request.range);
      }
    }, delay);
  }

  function returnToStart() {
    return seek(0);
  }

  function seek(positionNs: number) {
    return run(async () => {
      await invoke("seek_replay", { positionNs: clampSeekPosition(positionNs, playback.duration_ns) });
      playback = await invoke<PlaybackState>("playback_state");
      await refreshPlaybackReadouts(playback.position_ns, session?.signals.map((signal) => signal.id) ?? graphSignalIds);
    });
  }

  function scrubWheel(deltaY: number) {
    if (!session) return;
    const base = scrubPending ?? scrubInFlight ?? playback.position_ns;
    const result = scrubTimeWindow(base, deltaY, timeWindow, playback.duration_ns);
    scrubTo(result.positionNs, result.range);
  }

  function scrubTo(positionNs: number, range = followPlayhead(timeWindow, positionNs, playback.duration_ns)) {
    scrubPending = clampSeekPosition(positionNs, playback.duration_ns);
    playback = { ...playback, position_ns: scrubPending };
    timeWindow = range;
    if (scrubInFlight === null) flushScrub();
  }

  async function flushScrub() {
    while (scrubPending !== null) {
      const info = session;
      const positionNs = scrubPending;
      const signalIds = info?.signals.map((signal) => signal.id) ?? graphSignalIds;
      scrubPending = null;
      scrubInFlight = positionNs;
      try {
        await invoke("seek_replay", { positionNs });
        const [state, values] = await Promise.all([
          invoke<PlaybackState>("playback_state"),
          invoke<Sample[]>("recording_values_at", { signalIds, positionNs }),
        ]);
        if (session === info && scrubPending === null) {
          playback = state;
          latest = Object.fromEntries(values.map((sample) => [sample.signal_id, sample]));
          if (playback.playing && session) timeWindow = followPlayhead(timeWindow, playback.position_ns, playback.duration_ns);
        }
      } catch (reason) {
        showError(reason);
      } finally {
        scrubInFlight = null;
      }
    }
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
      if (timeWindow.endNs > playback.duration_ns) {
        const endNs = Math.min(timeWindow.endNs, playback.duration_ns);
        const startNs = Math.min(timeWindow.startNs, endNs);
        timeWindow = startNs < endNs
          ? { startNs, endNs }
          : { startNs: 0, endNs: playback.duration_ns };
      }
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

  function toggleLogSignal(signalId: string, enabled: boolean) {
    ({ logSignalIds, graphSignalIds } = setLogSignal(logSignalIds, graphSignalIds, signalId, enabled));
  }

  function toggleGraphSignal(signalId: string, enabled: boolean) {
    graphSignalIds = setGraphSignal(graphSignalIds, signalId, enabled);
    if (session && !playback.playing) {
      refreshPlaybackReadouts(playback.position_ns, session.signals.map((signal) => signal.id)).catch(showError);
    }
  }

  function updateSignalColor(signalId: string, color: string) {
    signalColors = { ...signalColors, [signalId]: color };
    try { localStorage.setItem("lognook.signal-colors", JSON.stringify(signalColors)); } catch { /* Preferences are optional. */ }
  }

  function updateWheelScrubbing(event: Event) {
    wheelScrubbing = (event.currentTarget as HTMLInputElement).checked;
    try { localStorage.setItem("lognook.wheel-scrubbing", String(wheelScrubbing)); } catch { /* Preferences are optional. */ }
  }

  function swapGraphPositions(fromId: string, toId: string) {
    graphSignalIds = swapPositions(graphSignalIds, graphSignalIds.indexOf(fromId), graphSignalIds.indexOf(toId));
  }

  function resizeStatus(event: PointerEvent) {
    if (!statusResize) return;
    statusHeight = Math.max(36, Math.min(window.innerHeight * 0.3, statusResize.height + statusResize.y - event.clientY));
  }

  function finishStatusResize() {
    statusResize = null;
  }

  function resizeStatusFromKeyboard(event: KeyboardEvent) {
    if (event.key !== "ArrowUp" && event.key !== "ArrowDown") return;
    event.preventDefault();
    statusHeight = Math.max(36, Math.min(window.innerHeight * 0.3, statusHeight + (event.key === "ArrowUp" ? 16 : -16)));
  }

  function sampleRate(signalId: string): string {
    const points = session ? sessionSeries[signalId] ?? [] : series[signalId] ?? [];
    if (points.length < 2) return "— Hz";
    const previous = points.at(-2)!;
    const current = points.at(-1)!;
    const delta = current.elapsed_ns - previous.elapsed_ns;
    return delta > 0 ? `${(1e9 / delta).toFixed(1)} Hz` : "— Hz";
  }

  function formatSignalValue(value: number | undefined, unit: string): string {
    return value === undefined ? `— ${unit}` : `${value.toFixed(1)} ${unit}`;
  }
</script>

<svelte:head>
  <title>LogNook · Acquisition</title>
  <meta name="description" content="Read-only OBD acquisition, recording, and CSV playback." />
</svelte:head>

<div class="app-shell" style={`--status-height:${statusHeight}px`}>
  <header class="topbar">
    <a class="brand" href="/" aria-label="LogNook home"><span class="brand-mark">L</span><span>Log<span class="brand-accent">Nook</span></span></a>
    <div class="connection-state" aria-label={status} title={status}><span class:online={connected} class:replay={replaying} class="state-dot"></span></div>
    <details class="settings-menu">
      <summary class="icon-button" aria-label="Settings" title="Settings">⚙</summary>
      <div class="settings-popover">
        <strong>Settings</strong>
        <label><input type="checkbox" checked={wheelScrubbing} onchange={updateWheelScrubbing} /> Wheel scrubs graph</label>
        <small>Replay keeps the playhead centered while playing.</small>
      </div>
    </details>
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
          <button class="button secondary" onclick={connectSx} disabled={busy || !selectedSxPort || sxIdentity?.path !== selectedSxPort || logSignalIds.length === 0 || replaying}>Connect OBDLink SX</button>
          <button class="button primary" onclick={connect} disabled={busy || logSignalIds.length === 0 || replaying}>Connect simulator</button>
        {/if}
        {#if recording}
          <button class="button danger" onclick={stopRecording} disabled={busy}>Stop recording</button>
        {:else}
          <button class="button record" onclick={startRecording} disabled={busy || !connected || replaying}>● Record</button>
        {/if}
      </div>
    </section>


    <PlaybackPanel
      info={session}
      {playback}
      {markers}
      range={timeWindow}
      openDisabled={connected}
      disabled={busy || recording}
      onOpen={openCsv}
      onClose={closeSession}
      onTogglePlay={togglePlayback}
      onStart={returnToStart}
      onScrub={scrubTo}
      onSpeed={setSpeed}
      onAddMarker={addMarker}
      onUpdateMarker={updateMarker}
      onDeleteMarker={deleteMarker}
      onJumpMarker={seek}
      onZoom={zoom}
      onPan={pan}
      onReset={resetZoom}
    />

    <div class="workspace-grid" class:pid-picker-hidden={!signalPickerVisible}>
      <button class="pid-list-toggle" aria-controls="signal-picker" aria-expanded={signalPickerVisible} onclick={() => signalPickerVisible = !signalPickerVisible}>{signalPickerVisible ? "Hide List" : "Show List"}</button>
      <aside id="signal-picker" class="panel signal-panel" hidden={!signalPickerVisible}>
        <div class="panel-heading"><div><p class="eyebrow">SIGNAL PICKER</p><h2>{session ? "Recorded signals" : "Engine · Mode 01"}</h2></div><span class="count-pill">{session ? `${graphSignalIds.length} graphed` : `${logSignalIds.length} logging · ${graphSignalIds.length} graphed`}</span></div>
        <input class="text-input signal-search" type="search" bind:value={signalSearch} placeholder="Search PID, name or unit" aria-label="Search signals" />
        <div class="signal-columns" aria-hidden="true"><span></span><span></span><span>Log</span><span>Graph</span></div>
        <div class="signal-list">
          {#each filteredSignals as signal (signal.id)}
            <div class="signal-option">
              <input class="signal-color" type="color" value={signalColor(signal.id, availableSignals.indexOf(signal), signalColors)} aria-label={`Color for ${signal.name}`} onchange={(event) => updateSignalColor(signal.id, event.currentTarget.value)} />
              <span class="signal-copy">
                <span class="signal-name-row"><span>{signal.name}</span><strong>{formatSignalValue(latest[signal.id]?.value, signal.unit)}</strong></span>
                <small>{signal.pid === undefined ? signal.id : `PID ${signal.pid.toString(16).toUpperCase().padStart(2, "0")}`} · avg {formatSignalValue(signalAverages[signal.id], signal.unit)}</small>
              </span>
              <input type="checkbox" aria-label={`Log ${signal.name}`} checked={session ? true : logSignalIds.includes(signal.id)} onchange={(event) => toggleLogSignal(signal.id, event.currentTarget.checked)} disabled={session !== null || connected || recording || busy} />
              <input type="checkbox" aria-label={`Graph ${signal.name}`} checked={graphSignalIds.includes(signal.id)} onchange={(event) => toggleGraphSignal(signal.id, event.currentTarget.checked)} disabled={busy || (!session && !logSignalIds.includes(signal.id))} />
            </div>
          {:else}
            <p class="no-signals">{availableSignals.length ? "No matching signals" : "Loading signals…"}</p>
          {/each}
        </div>
        <p class="panel-hint">{session ? "Recorded signals are fixed; Graph controls visibility." : "Logging acquires the PID and adds it to Graph. Hide a trace while continuing to log it."}</p>
      </aside>

      <section class="measurements" aria-label="Measurements">
        {#if visibleSignals.length === 0}
          <div class="empty-state"><span class="empty-icon">⌁</span><h3>{session ? "No signals selected" : "Select signals to begin"}</h3><p>{session ? "Choose recorded signals to add them to the graph." : "Choose one or more PIDs, then connect an adapter."}</p></div>
        {:else}
          <SignalGraphs
            {traces}
            {graphMode}
            onReorder={swapGraphPositions}
            onSeek={session ? seek : undefined}
            onWheelScrub={session && wheelScrubbing ? scrubWheel : undefined}
            timeWindow={session ? timeWindow : undefined}
            markers={session ? markers : []}
            playheadNs={session ? playback.position_ns : undefined}
          />
        {/if}
      </section>
    </div>
  </main>
  <footer class="status-dock" aria-live="polite">
    <div class="status-resize-grip" role="slider" tabindex="0" aria-orientation="vertical" aria-valuemin="36" aria-valuemax={Math.round(window.innerHeight * 0.3)} aria-valuenow={Math.round(statusHeight)} aria-label="Resize status panel"
      onpointerdown={(event) => { statusResize = { y: event.clientY, height: statusHeight }; event.currentTarget.setPointerCapture(event.pointerId); }}
      onpointermove={resizeStatus} onpointerup={finishStatusResize} onpointercancel={finishStatusResize} onkeydown={resizeStatusFromKeyboard}></div>
    <div class="status-content">
      <strong>{status}</strong>
      {#if error}<span class="status-error" role="alert">{error}</span>{/if}
      {#if sxIdentity}<span class="status-success">Identified {sxIdentity.hardware_family} · {sxIdentity.firmware} · serial {sxIdentity.stable_id}</span>{/if}
      {#if recording}<span class="status-recording"><span class="pulse-dot"></span>Recording live samples · {recordingPath}</span>{/if}
      {#if exportPath}<span class="status-success">CSV exported · {exportPath}</span>{/if}
    </div>
  </footer>
</div>
