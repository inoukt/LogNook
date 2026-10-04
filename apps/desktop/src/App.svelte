<script lang="ts">
  import { onMount, tick } from "svelte";
  import { invoke, isTauri } from "@tauri-apps/api/core";
  import { getCurrentWebview } from "@tauri-apps/api/webview";
  import { liveCsvSamples } from "./liveCsv";
  import { listen, type UnlistenFn } from "@tauri-apps/api/event";
  import DerivedSignalsPanel from "./DerivedSignalsPanel.svelte";
  import PlaybackPanel from "./PlaybackPanel.svelte";
  import SessionBrowser from "./SessionBrowser.svelte";
  import { openAndActivateRecording } from "./sessionBrowser";
  import { adapterLabel, connectionControlState, type AdapterKind } from "./connectionControls";
  import AutomaticTriggerPanel from "./AutomaticTriggerPanel.svelte";
  import SettingsPanel from "./SettingsPanel.svelte";
  import SignalGraphs from "./SignalGraphs.svelte";
  import OemCatalogPanel from "./OemCatalogPanel.svelte";
  import {
    defaultAutomaticTriggerConfig, disarmedAutomaticTriggerStatus, normalizeAutomaticTriggerConfig,
    restoreAutomaticTriggerConfig, type AutomaticTriggerConfig, type AutomaticTriggerStatus,
  } from "./automaticTrigger";
  import {
    derivedSignalOptions, formulaSignalIds, graphScaleSignalIds, restoreDerivedSignalDefinitions,
    type DerivedSignalDefinition, type DerivedSignalSource,
  } from "./derivedSignals";
  import type { ShortcutAction } from "./shortcuts";
  import type {
    ChartTrace, ConnectionCapabilities, DeviceDescriptor, EngineEvent, Marker, PlaybackState,
    J2534Provider, RecordingInfo, Sample, SignalDefinition,
  } from "./types";
  import {
    averageInWindow, clampSeekPosition, filterSignals, followPlayhead, graphRefreshIntervalMs, orderMarkersForTimeline,
    panTimeWindow, restoreWorkspacePreferences, sampleTimeRange, scrubTimeWindow, setGraphSignal,
    setLogSignal, signalColor, swapPositions, zoomTimeWindow, type SignalOption, type TimeWindow,
    type WorkspacePreferences, type WorkspaceTheme,
  } from "./workspace";

  const defaultWorkspacePreferences: WorkspacePreferences = {
    logSignalIds: ["engine_rpm", "coolant_temp", "intake_temp"],
    graphSignalIds: ["engine_rpm", "coolant_temp", "intake_temp"],
    graphMode: "overlay",
    signalPickerVisible: true,
    statusHeight: 42,
    theme: "midnight",
    highContrast: false,
    uiScale: 100,
  };
  const workspacePreferencesKey = "lognook.workspace-preferences";
  const automaticTriggerPreferencesKey = "lognook.automatic-trigger";

  let signalCatalog: SignalDefinition[] = [];
  let derivedSignalDefinitions: DerivedSignalDefinition[] = [];
  let sxPorts: string[] = [];
  let selectedSxPort = "";
  let sxIdentity: DeviceDescriptor | undefined;
  let openPortProviders: J2534Provider[] = [];
  let selectedOpenPortProvider = "";
  let openPortIdentity: DeviceDescriptor | undefined;
  let selectedAdapter: AdapterKind = "simulator";
  let logSignalIds = [...defaultWorkspacePreferences.logSignalIds];
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
  let automaticRecording = false;
  let automaticTriggerConfig: AutomaticTriggerConfig = { ...defaultAutomaticTriggerConfig };
  let automaticTriggerStatus: AutomaticTriggerStatus = { ...disarmedAutomaticTriggerStatus };
  let automaticTriggerPreferencesReady = false;
  let derivedSignalPreferencesReady = false;
  let replaying = false;
  let busy = false;
  let status = "Disconnected";
  let recordingPath = "";
  let exportPath = "";
  let markerNote = "";
  let error = "";
  let signalSearch = "";
  let signalSearchInput: HTMLInputElement | undefined;
  let derivedSignalDialog: HTMLDialogElement | undefined;
  let derivedSignalSource: DerivedSignalSource | null = null;
  let signalPickerVisible = true;
  let pidPickerView: "signals" | "oem" = "signals";
  let theme: WorkspaceTheme = defaultWorkspacePreferences.theme;
  let highContrast = defaultWorkspacePreferences.highContrast;
  let uiScale = defaultWorkspacePreferences.uiScale;
  let sessionBrowserOpen = false;
  let wheelScrubbing = false;
  let workspacePreferencesReady = false;
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
  let availableSignals: SignalOption[] = [];
  $: connectionControls = connectionControlState(selectedAdapter, {
    busy,
    recording,
    replaying,
    hasSignals: logSignalIds.length > 0,
    sxPort: selectedSxPort,
    sxIdentityMatches: sxIdentity?.path === selectedSxPort,
    openPortProvider: selectedOpenPortProvider,
    openPortIdentityMatches: openPortIdentity?.stable_id === selectedOpenPortProvider,
  });

  $: derivedOptions = derivedSignalOptions(derivedSignalDefinitions);
  $: availableSignals = session
    ? session.signals.map((recorded): SignalOption => {
        const known = signalCatalog.find((signal) => signal.id === recorded.id)
          ?? derivedOptions.find((signal) => signal.id === recorded.id);
        return known ?? {
          id: recorded.id,
          name: recorded.id,
          unit: recorded.unit,
          description: "Recorded signal",
        };
      })
    : [...signalCatalog, ...derivedOptions];
  $: visibleSignals = graphSignalIds.flatMap((id) => {
    const signal = availableSignals.find((candidate) => candidate.id === id);
    return signal ? [signal] : [];
  });
  $: graphWindowSignalIds = [...new Set([
    ...graphSignalIds,
    ...visibleSignals.flatMap((signal) => formulaSignalIds(signal.formula, signalCatalog)),
  ])];
  $: filteredSignals = filterSignals(availableSignals, signalSearch);
  $: hasLiveCsvSamples = logSignalIds.some((id) => (series[id]?.length ?? 0) > 0);
  $: automaticTriggerSignals = session ? [] : availableSignals.filter((signal) => logSignalIds.includes(signal.id));
  $: currentDerivedValues = Object.fromEntries(derivedSignalDefinitions.map((definition) => [
    definition.id,
    latest[definition.id]?.value,
  ]));
  $: if (!automaticTriggerStatus.armed && automaticTriggerSignals.length
    && !automaticTriggerSignals.some((signal) => signal.id === automaticTriggerConfig.signal_id)) {
    updateAutomaticTriggerConfig(automaticTriggerConfig);
  }
  $: liveGraphRange = sampleTimeRange(visibleSignals.flatMap((signal) => series[signal.id] ?? []));
  $: signalAverages = session
    ? sessionAverages
    : Object.fromEntries(availableSignals.map((signal) => [
        signal.id,
        averageInWindow(series[signal.id] ?? [], liveGraphRange),
      ]));
  $: markers = orderMarkersForTimeline(session?.markers ?? []);
  $: traces = visibleSignals.map((signal): ChartTrace => {
    const sourceSignalIds = formulaSignalIds(signal.formula, signalCatalog);
    const scaleSignalIds = graphScaleSignalIds(signal.id, visibleSignals, availableSignals);
    const samplesBySignal = session ? sessionSeries : series;
    return {
      signalId: signal.id,
      name: signal.name,
      pid: signal.pid,
      unit: signal.unit,
      color: signalColorFor(signal),
      formula: signal.formula,
      sourceSignalIds,
      scaleSamples: scaleSignalIds.filter((id) => id !== signal.id).flatMap((id) => samplesBySignal[id] ?? []),
      samples: samplesBySignal[signal.id] ?? [],
      current: latest[signal.id],
      sampleRate: sampleRate(signal.id),
    };
  });
  $: if (session) scheduleGraphWindow(session, graphWindowSignalIds, timeWindow);
  $: if (workspacePreferencesReady) {
    try {
      localStorage.setItem(workspacePreferencesKey, JSON.stringify({
        ...defaultWorkspacePreferences,
        logSignalIds,
        graphSignalIds: liveGraphSignalIds ?? graphSignalIds,
        graphMode,
        signalPickerVisible,
        statusHeight,
        theme,
        highContrast,
        uiScale,
      }));
    } catch { /* Preferences are optional. */ }
  }
  $: if (workspacePreferencesReady && isTauri()) {
    getCurrentWebview().setZoom(uiScale / 100).catch(showError);
  }
  $: if (automaticTriggerPreferencesReady) {
    try { localStorage.setItem(automaticTriggerPreferencesKey, JSON.stringify(automaticTriggerConfig)); }
    catch { /* Preferences are optional. */ }
  }
  $: if (derivedSignalPreferencesReady) {
    try { localStorage.setItem("lognook.derived-signals", JSON.stringify(derivedSignalDefinitions)); }
    catch { /* Preferences are optional. */ }
  }

  onMount(() => {
    let disposed = false;
    let savedWorkspacePreferences: string | null = null;
    let savedAutomaticTrigger: string | null = null;
    let savedDerivedSignals: string | null = null;
    try {
      savedWorkspacePreferences = localStorage.getItem(workspacePreferencesKey);
      savedAutomaticTrigger = localStorage.getItem(automaticTriggerPreferencesKey);
      savedDerivedSignals = localStorage.getItem("lognook.derived-signals");
    } catch { /* Preferences are optional. */ }
    try {
      const colors = JSON.parse(localStorage.getItem("lognook.signal-colors") ?? "{}");
      if (colors && typeof colors === "object" && !Array.isArray(colors)) signalColors = colors;
    } catch { /* Ignore unavailable or malformed local preferences. */ }
    listen<EngineEvent>("lognook-event", ({ payload }) => handleEvent(payload))
      .then((stop) => disposed ? stop() : unlisten = stop)
      .catch(showError);
    invoke<SignalDefinition[]>("available_signals")
      .then((signals) => {
        signalCatalog = signals;
        derivedSignalDefinitions = restoreDerivedSignalDefinitions(savedDerivedSignals, signals.map(({ id }) => id));
        derivedSignalPreferencesReady = true;
        const preferences = restoreWorkspacePreferences(
          savedWorkspacePreferences,
          [...signals.map(({ id }) => id), ...derivedSignalDefinitions.map(({ id }) => id)],
          defaultWorkspacePreferences,
          Math.max(36, window.innerHeight * 0.3),
        );
        logSignalIds = preferences.logSignalIds;
        automaticTriggerConfig = restoreAutomaticTriggerConfig(
          savedAutomaticTrigger,
          preferences.logSignalIds,
          preferences.logSignalIds[0] ?? signals[0]?.id ?? defaultAutomaticTriggerConfig.signal_id,
        );
        automaticTriggerPreferencesReady = true;
        if (session) liveGraphSignalIds = preferences.graphSignalIds;
        else graphSignalIds = preferences.graphSignalIds;
        graphMode = preferences.graphMode;
        signalPickerVisible = preferences.signalPickerVisible;
        statusHeight = preferences.statusHeight;
        theme = preferences.theme;
        highContrast = preferences.highContrast;
        uiScale = preferences.uiScale;
        workspacePreferencesReady = true;
      })
      .catch(() => error = "Open LogNook as a desktop app to access the acquisition engine.");
    invoke<string[]>("available_obdlink_sx_ports")
      .then((ports) => {
        sxPorts = ports;
        selectedSxPort = ports[0] ?? "";
      })
      .catch(() => sxPorts = []);
    invoke<AutomaticTriggerStatus>("automatic_trigger_status")
      .then((triggerStatus) => handleEvent({ type: "triggerStatus", payload: triggerStatus }))
      .catch(() => {});
    return () => {
      disposed = true;
      unlisten?.();
    };
  });

  function runShortcut(action: ShortcutAction) {
    if (action === "toggleRecording") {
      if (busy) return;
      if (recording) stopRecording();
      else if (connected && !replaying) startRecording();
    } else if (action === "togglePlayback") {
      if (session && !recording && !busy) togglePlayback();
    } else if (action === "insertMarker") {
      if (busy) return;
      if (recording) insertLiveMarker();
      else if (session) addMarker("Manual marker", "");
    } else if (action === "previousMarker") {
      jumpToMarker(-1);
    } else if (action === "nextMarker") {
      jumpToMarker(1);
    } else if (action === "focusPidSearch") {
      focusPidSearch();
    } else if (action === "captureScreenshot") {
      window.dispatchEvent(new Event("lognook-capture-screenshot"));
    } else if (action === "zoomIn") {
      zoom(2);
    } else if (action === "zoomOut") {
      zoom(0.5);
    }
  }

  function jumpToMarker(direction: -1 | 1) {
    if (!session || busy) return;
    const marker = direction < 0
      ? [...markers].reverse().find((item) => item.elapsed_ns < playback.position_ns)
      : markers.find((item) => item.elapsed_ns > playback.position_ns);
    if (marker) seek(marker.elapsed_ns);
  }

  async function focusPidSearch() {
    if (!signalPickerVisible) signalPickerVisible = true;
    await tick();
    signalSearchInput?.focus();
  }

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
    } else if (event.type === "triggerStatus") {
      automaticTriggerStatus = event.payload;
      if (event.payload.armed) status = "Automatic trigger armed";
      if (event.payload.recording) {
        automaticRecording = true;
        recording = true;
        recordingPath = event.payload.recording_path ?? "automatic trigger";
        status = "Automatic trigger fired · recording";
      } else if (automaticRecording) {
        automaticRecording = false;
        recording = false;
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
      automaticRecording = false;
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
    sessionBrowserOpen = false;
    recordingPath = "loaded";
    exportPath = "";
    replaying = false;
    if (!connected) status = "Session loaded";
    signalSearch = "";
  }

  function openSavedRecording(path: string) {
    return run(() => openAndActivateRecording(
      () => invoke<RecordingInfo>("open_recording", { path }),
      activateSession,
    ));
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
      const requested = logSignalIds.filter((id) => availableSignals.some((signal) => signal.id === id));
      if (!requested.length) throw new Error("Select at least one supported engine signal first");
      await invoke("configure_derived_signals", {
        definitions: derivedSignalDefinitions,
        activeIds: derivedSignalDefinitions.filter((definition) => requested.includes(definition.id)).map(({ id }) => id),
      });
      const capabilities = await invoke<ConnectionCapabilities>("connect_simulator", { signalIds: requested });
      await clearSession();
      series = {};
      latest = {};
      const availableIds = new Set([...capabilities.supported_signals.map(({ id }) => id), ...derivedSignalDefinitions.map(({ id }) => id)]);
      logSignalIds = logSignalIds.filter((id) => availableIds.has(id));
      graphSignalIds = graphSignalIds.filter((id) => availableIds.has(id));
    });
  }

  function connectSx() {
    return run(async () => {
      const requested = logSignalIds.filter((id) => availableSignals.some((signal) => signal.id === id));
      if (!requested.length) throw new Error("Select at least one supported engine signal first");
      await invoke("configure_derived_signals", {
        definitions: derivedSignalDefinitions,
        activeIds: derivedSignalDefinitions.filter((definition) => requested.includes(definition.id)).map(({ id }) => id),
      });
      const capabilities = await invoke<ConnectionCapabilities>("connect_obdlink_sx", {
        portName: selectedSxPort,
        signalIds: requested,
      });
      await clearSession();
      series = {};
      latest = {};
      const availableIds = new Set([...capabilities.supported_signals.map(({ id }) => id), ...derivedSignalDefinitions.map(({ id }) => id)]);
      logSignalIds = logSignalIds.filter((id) => availableIds.has(id));
      graphSignalIds = graphSignalIds.filter((id) => availableIds.has(id));
    });
  }

  function connectOpenPort() {
    return run(async () => {
      const requested = logSignalIds.filter((id) => availableSignals.some((signal) => signal.id === id));
      if (!requested.length) throw new Error("Select at least one supported engine signal first");
      await invoke("configure_derived_signals", {
        definitions: derivedSignalDefinitions,
        activeIds: derivedSignalDefinitions.filter((definition) => requested.includes(definition.id)).map(({ id }) => id),
      });
      const capabilities = await invoke<ConnectionCapabilities>("connect_openport2", {
        providerId: selectedOpenPortProvider,
        signalIds: requested,
      });
      await clearSession();
      series = {};
      latest = {};
      const availableIds = new Set([...capabilities.supported_signals.map(({ id }) => id), ...derivedSignalDefinitions.map(({ id }) => id)]);
      logSignalIds = logSignalIds.filter((id) => availableIds.has(id));
      graphSignalIds = graphSignalIds.filter((id) => availableIds.has(id));
    });
  }

  function connectSelectedAdapter() {
    if (selectedAdapter === "obdlink_sx") return connectSx();
    if (selectedAdapter === "openport2") return connectOpenPort();
    return connect();
  }

  function selectAdapter(adapter: AdapterKind) {
    selectedAdapter = adapter;
    if (adapter !== "openport2") return;
    openPortProviders = [];
    selectedOpenPortProvider = "";
    openPortIdentity = undefined;
    void run(async () => {
      openPortProviders = await invoke<J2534Provider[]>("available_openport2_providers");
      selectedOpenPortProvider = openPortProviders[0]?.id ?? "";
    });
  }

  function identifySx() {
    return run(async () => {
      sxIdentity = undefined;
      sxIdentity = await invoke<DeviceDescriptor>("identify_obdlink_sx", { portName: selectedSxPort });
    });
  }

  function identifyOpenPort() {
    return run(async () => {
      openPortIdentity = undefined;
      openPortIdentity = await invoke<DeviceDescriptor>("identify_openport2", {
        providerId: selectedOpenPortProvider,
      });
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
      automaticRecording = false;
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

  function exportLiveCsv() {
    return run(async () => {
      const samples = liveCsvSamples(series, logSignalIds);
      const path = await invoke<string | null>("export_live_csv", { samples });
      if (path) {
        exportPath = path;
        status = `Saved ${samples.length.toLocaleString()} buffered samples`;
      }
    });
  }

  function shareRecording(path: string) {
    return run(async () => {
      const destination = await invoke<string | null>("share_recording_csv", { path });
      if (destination) {
        exportPath = destination;
        status = "Shareable CSV saved";
      }
    });
  }

  function saveRecordingAs(path: string) {
    return run(async () => {
      const destination = await invoke<string | null>("save_recording_as", { path });
      if (destination) status = "Session copy saved";
    });
  }

  function openSessionFile() {
    return run(async () => {
      const info = await invoke<RecordingInfo | null>("open_session_file");
      if (info) await activateSession(info);
    });
  }

  function updateAutomaticTriggerConfig(config: AutomaticTriggerConfig) {
    automaticTriggerConfig = normalizeAutomaticTriggerConfig(
      config,
      automaticTriggerSignals.map((signal) => signal.id),
      automaticTriggerSignals[0]?.id ?? defaultAutomaticTriggerConfig.signal_id,
    );
  }

  function armAutomaticTrigger() {
    return run(async () => {
      updateAutomaticTriggerConfig(automaticTriggerConfig);
      automaticTriggerStatus = await invoke<AutomaticTriggerStatus>("arm_automatic_trigger", {
        config: automaticTriggerConfig,
      });
      status = "Automatic trigger armed";
    });
  }

  function disarmAutomaticTrigger() {
    return run(async () => {
      automaticTriggerStatus = await invoke<AutomaticTriggerStatus>("disarm_automatic_trigger");
      status = connected ? "Automatic trigger disarmed" : "Disconnected";
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
    const scrubbing = scrubPending !== null || scrubInFlight !== null;
    const delay = Math.max(0, graphRefreshIntervalMs(scrubbing) - (performance.now() - lastGraphRefreshAt));
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

  function updateDerivedSignals(definitions: DerivedSignalDefinition[]) {
    const oldIds = new Set(derivedSignalDefinitions.map(({ id }) => id));
    const nextIds = new Set(definitions.map(({ id }) => id));
    const added = definitions.filter(({ id }) => !oldIds.has(id)).map(({ id }) => id);
    const keepSelected = (ids: string[]) => ids.filter((id) => !oldIds.has(id) || nextIds.has(id));
    logSignalIds = [...new Set([...keepSelected(logSignalIds), ...added])];
    graphSignalIds = [...new Set([...keepSelected(graphSignalIds), ...added])];
    derivedSignalDefinitions = definitions;
  }

  function openDerivedSignalEditor(source: DerivedSignalSource) {
    derivedSignalSource = source;
    derivedSignalDialog?.showModal();
  }

  function tareDerivedSignal(signalId: string) {
    return run(async () => {
      const tareOffset = await invoke<number>("tare_derived_signal", { signalId });
      derivedSignalDefinitions = derivedSignalDefinitions.map((definition) => definition.id === signalId
        ? { ...definition, tare_offset: tareOffset }
        : definition);
    });
  }

  function updateSignalColor(signalId: string, color: string) {
    signalColors = { ...signalColors, [signalId]: color };
    try { localStorage.setItem("lognook.signal-colors", JSON.stringify(signalColors)); } catch { /* Preferences are optional. */ }
  }

  function resetPreferences() {
    const preferences = restoreWorkspacePreferences(null, signalCatalog.map(({ id }) => id), defaultWorkspacePreferences, Math.max(36, window.innerHeight * 0.3));
    logSignalIds = preferences.logSignalIds;
    if (session) liveGraphSignalIds = preferences.graphSignalIds;
    else graphSignalIds = preferences.graphSignalIds;
    graphMode = preferences.graphMode;
    signalPickerVisible = preferences.signalPickerVisible;
    statusHeight = preferences.statusHeight;
    theme = preferences.theme;
    highContrast = preferences.highContrast;
    uiScale = preferences.uiScale;
    signalColors = {};
    wheelScrubbing = false;
    automaticTriggerConfig = { ...defaultAutomaticTriggerConfig };
    derivedSignalDefinitions = [];
    try {
      localStorage.removeItem(workspacePreferencesKey);
      localStorage.removeItem("lognook.signal-colors");
      localStorage.removeItem("lognook.wheel-scrubbing");
      localStorage.removeItem("lognook.automatic-trigger");
      localStorage.removeItem("lognook.derived-signals");
      localStorage.removeItem("lognook.graph-preferences.v1");
    } catch { /* Preferences are optional. */ }
    window.dispatchEvent(new Event("lognook-reset-graph-preferences"));
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

  function signalColorFor(signal: SignalOption): string {
    const avoidColors = formulaSignalIds(signal.formula, availableSignals).flatMap((id) => {
      const source = availableSignals.find((candidate) => candidate.id === id);
      return source ? [signalColor(source.id, availableSignals.indexOf(source), signalColors)] : [];
    });
    return signalColor(signal.id, availableSignals.indexOf(signal), signalColors, avoidColors);
  }
</script>

<svelte:head>
  <title>LogNook · Acquisition</title>
  <meta name="description" content="Read-only OBD acquisition, recording, and CSV playback." />
</svelte:head>

<div class="app-shell" data-theme={theme} data-high-contrast={highContrast} style={`--status-height:${statusHeight}px;--ui-scale-factor:${uiScale / 100}`}>
  <header class="topbar">
    <a class="brand" href="/" aria-label="LogNook home"><span class="brand-mark">L</span><span>Log<span class="brand-accent">Nook</span></span></a>
    <div class="connection-state" aria-label={status} title={status}><span class:online={connected} class:replay={replaying} class="state-dot"></span></div>
    <SettingsPanel
      bind:wheelScrubbing
      bind:theme
      bind:highContrast
      bind:uiScale
      resetDisabled={!workspacePreferencesReady || connected || recording || replaying || session !== null}
      onAction={runShortcut}
      onReset={resetPreferences}
    />
    <div class="session-tools">
      <details class="session-browser-details topbar-session-browser" bind:open={sessionBrowserOpen}>
        <summary aria-label="Browse saved sessions" title="Browse saved sessions">Browse saved sessions</summary>
        <SessionBrowser open={sessionBrowserOpen} disabled={busy || recording} openDisabled={connected || recording} onOpen={openSavedRecording} onShare={shareRecording} onSaveAs={saveRecordingAs} />
      </details>
      {#if !session}
        <button class="button secondary open-csv-compact" title="Open CSV or LogNook session" aria-label="Open session file" onclick={openSessionFile} disabled={busy || connected || recording}>CSV</button>
      {/if}
    </div>
    <span class="milestone">OBDLinkSX-Beta1</span>
  </header>

  <main>
    <section class="workspace-toolbar" aria-label="Acquisition controls">
      <div class="workspace-title"><p class="eyebrow">LIVE WORKSPACE</p><h1>Acquisition</h1></div>
      <div class="toolbar-groups">
        <div class="toolbar-group utility-group" role="group" aria-label="Graph and data tools">
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
            {:else if connected && !session}
              <button class="button secondary" onclick={exportLiveCsv} disabled={busy || !hasLiveCsvSamples} title="Save the retained live samples for logged signals as CSV">Save CSV</button>
            {:else}
              <button class="button secondary" onclick={exportCsv} disabled={busy || (!recordingPath && !session)} title="Export recorded data as CSV" aria-label="Export CSV">Export</button>
            {/if}
          </div>
        </div>
        <span class="toolbar-divider" aria-hidden="true"></span>
        <div class="toolbar-group connection-actions" role="group" aria-label="Connection and recording controls">
          {#if connected}
            <button class="button secondary" onclick={disconnect} disabled={busy}>Disconnect</button>
          {:else}
            <select class="text-input adapter-select" value={selectedAdapter} onchange={(event) => selectAdapter(event.currentTarget.value as AdapterKind)} aria-label="Adapter" disabled={busy || recording || replaying}>
              <option value="simulator">Simulator</option>
              <option value="obdlink_sx">OBDLink SX</option>
              <option value="openport2">OpenPort 2</option>
            </select>
            {#if selectedAdapter === "obdlink_sx"}
              <select class="text-input port-select" bind:value={selectedSxPort} onchange={() => sxIdentity = undefined} aria-label="OBDLink SX serial port" disabled={busy || recording || sxPorts.length === 0 || replaying}>
                {#if sxPorts.length === 0}<option value="">No SX found</option>{/if}
                {#each sxPorts as port}<option value={port}>{port}</option>{/each}
              </select>
            {:else if selectedAdapter === "openport2"}
              <select class="text-input provider-select" bind:value={selectedOpenPortProvider} onchange={() => openPortIdentity = undefined} aria-label="OpenPort J2534 provider" disabled={busy || recording || openPortProviders.length === 0 || replaying}>
                {#if openPortProviders.length === 0}<option value="">No OpenPort provider</option>{/if}
                {#each openPortProviders as provider}<option value={provider.id}>{provider.name}</option>{/each}
              </select>
            {/if}
            {#if selectedAdapter !== "simulator"}
              <button class="button secondary" onclick={selectedAdapter === "openport2" ? identifyOpenPort : identifySx} disabled={connectionControls.identifyDisabled} title={`Identify ${adapterLabel(selectedAdapter)}`} aria-label={`Identify ${adapterLabel(selectedAdapter)}`}>Identify</button>
              {#if (selectedAdapter === "obdlink_sx" && sxIdentity?.path === selectedSxPort) || (selectedAdapter === "openport2" && openPortIdentity?.stable_id === selectedOpenPortProvider)}
                <span class="adapter-identified" title={selectedAdapter === "openport2" ? openPortIdentity?.firmware : sxIdentity?.firmware}>Ready</span>
              {/if}
            {/if}
            <button class="button primary" onclick={connectSelectedAdapter} disabled={connectionControls.connectDisabled} title={connectionControls.connectLabel} aria-label={connectionControls.connectLabel}>Connect</button>
          {/if}
          {#if recording}
            <button class="button danger" onclick={stopRecording} disabled={busy} title="Stop recording" aria-label="Stop recording">Stop</button>
          {:else}
            <button class="button record" onclick={startRecording} disabled={busy || !connected || replaying}>● Record</button>
          {/if}
        </div>
      </div>
    </section>


    {#if session}
      <PlaybackPanel
        info={session}
        {playback}
        {markers}
        range={timeWindow}
        openDisabled={connected}
        disabled={busy || recording}
        onOpen={openSessionFile}
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
    {/if}

    <dialog class="derived-signal-dialog" aria-label="Derived signal editor" bind:this={derivedSignalDialog} onclose={() => derivedSignalSource = null}>
      <button class="derived-dialog-close" type="button" aria-label="Close derived signal editor" onclick={() => derivedSignalDialog?.close()}>×</button>
      <DerivedSignalsPanel
        definitions={derivedSignalDefinitions}
        baseSignalIds={signalCatalog.map(({ id }) => id)}
        sourceSignal={derivedSignalSource}
        currentValues={currentDerivedValues}
        disabled={busy || connected || recording || replaying || session !== null}
        tareDisabled={busy || !connected || replaying || session !== null}
        onChange={updateDerivedSignals}
        onTare={tareDerivedSignal}
      />
    </dialog>

    <div class="workspace-grid" class:pid-picker-hidden={!signalPickerVisible}>
      <button class="pid-list-toggle" aria-controls="signal-picker" aria-expanded={signalPickerVisible} onclick={() => signalPickerVisible = !signalPickerVisible}>{signalPickerVisible ? "Hide List" : "Show List"}</button>
      <aside id="signal-picker" class="panel signal-panel" hidden={!signalPickerVisible}>
        <div class="panel-heading"><div><p class="eyebrow">{session || pidPickerView === "signals" ? "SIGNAL PICKER" : "PID DATABASE"}</p><h2>{session ? "Recorded signals" : pidPickerView === "signals" ? "Engine · Mode 01" : "OEM research"}</h2></div><span class="count-pill">{session ? `${graphSignalIds.length} graphed` : pidPickerView === "signals" ? `${logSignalIds.length} logging · ${graphSignalIds.length} graphed` : "OEM catalog"}</span></div>
        {#if !session}
          <div class="pid-picker-tabs" role="group" aria-label="PID source">
            <button class:active={pidPickerView === "signals"} aria-pressed={pidPickerView === "signals"} onclick={() => pidPickerView = "signals"}>Live PIDs</button>
            <button class:active={pidPickerView === "oem"} aria-pressed={pidPickerView === "oem"} onclick={() => pidPickerView = "oem"}>OEM Research</button>
          </div>
        {/if}
        {#if session || pidPickerView === "signals"}
          <input class="text-input signal-search" type="search" bind:this={signalSearchInput} bind:value={signalSearch} placeholder="Search PID, name or unit" aria-label="Search signals" />
          <AutomaticTriggerPanel
            signals={automaticTriggerSignals}
            config={automaticTriggerConfig}
            status={automaticTriggerStatus}
            disabled={busy || !connected || recording || replaying || session !== null}
            onConfigChange={updateAutomaticTriggerConfig}
            onArm={armAutomaticTrigger}
            onDisarm={disarmAutomaticTrigger}
          />
          <div class="signal-columns" aria-hidden="true"><span></span><span></span><span>Log</span><span>Graph</span></div>
          <div class="signal-list">
            {#each filteredSignals as signal (signal.id)}
              <div class="signal-option">
                <input class="signal-color" type="color" value={signalColorFor(signal)} aria-label={`Color for ${signal.name}`} onchange={(event) => updateSignalColor(signal.id, event.currentTarget.value)} />
                <span class="signal-copy">
                  <span class="signal-name-row"><span title={signal.name}>{signal.name}</span><strong>{formatSignalValue(latest[signal.id]?.value, signal.unit)}</strong></span>
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
        {:else}
          <OemCatalogPanel selectedPort={selectedSxPort} disabled={busy || connected || recording || replaying || session !== null} />
        {/if}
      </aside>

      <section class="measurements" aria-label="Measurements">
        {#if visibleSignals.length === 0}
          <div class="empty-state"><span class="empty-icon">⌁</span><h3>{session ? "No signals selected" : "Select signals to begin"}</h3><p>{session ? "Choose recorded signals to add them to the graph." : "Choose one or more PIDs, then connect an adapter."}</p></div>
        {:else}
          <SignalGraphs
            {traces}
            {graphSignalIds}
            {graphMode}
            onReorder={swapGraphPositions}
            onCreateDerivedSignal={openDerivedSignalEditor}
            onToggleGraphSignal={toggleGraphSignal}
            onSeek={session ? seek : undefined}
            onWheelScrub={session && wheelScrubbing ? scrubWheel : undefined}
            live={!session && connected}
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
      {#if automaticTriggerStatus.armed}<span class="status-success">Automatic trigger armed · {automaticTriggerStatus.config?.signal_id} {automaticTriggerStatus.config?.direction} {automaticTriggerStatus.config?.threshold}</span>{/if}
      {#if recording}<span class="status-recording"><span class="pulse-dot"></span>{automaticRecording ? "Automatic trigger recording" : "Recording live samples"} · {recordingPath}</span>{/if}
      {#if exportPath}<span class="status-success">CSV exported · {exportPath}</span>{/if}
    </div>
  </footer>
</div>
