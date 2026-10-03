<script lang="ts">
  import type { SignalOption } from "./workspace";
  import type { AutomaticTriggerConfig, AutomaticTriggerStatus } from "./automaticTrigger";

  let {
    signals,
    config,
    status,
    disabled = false,
    onConfigChange,
    onArm,
    onDisarm,
  }: {
    signals: SignalOption[];
    config: AutomaticTriggerConfig;
    status: AutomaticTriggerStatus;
    disabled?: boolean;
    onConfigChange: (config: AutomaticTriggerConfig) => void;
    onArm: () => unknown;
    onDisarm: () => unknown;
  } = $props();

  let selectedSignal = $derived(signals.find((signal) => signal.id === config.signal_id));
  let locked = $derived(disabled || status.armed || status.recording);
  let statusLabel = $derived(status.recording
    ? "Recording · stop to finish"
    : status.armed ? "Armed · waiting for threshold" : "Disarmed");

  function updateNumber(field: "threshold" | "hysteresis" | "hold_ms" | "pre_trigger_ms", raw: string) {
    const value = Number(raw);
    if (Number.isFinite(value)) onConfigChange({ ...config, [field]: value });
  }
</script>

<details class="automatic-trigger-panel">
  <summary><span>⚡ Automatic trigger</span><small>{statusLabel}</small></summary>
  <div class="trigger-settings">
    <label>PID
      <select class="text-input" value={config.signal_id} onchange={(event) => onConfigChange({ ...config, signal_id: event.currentTarget.value })} disabled={locked || signals.length === 0}>
        {#each signals as signal (signal.id)}<option value={signal.id}>{signal.name} · {signal.unit}</option>{/each}
      </select>
    </label>
    <label>When
      <select class="text-input" value={config.direction} onchange={(event) => onConfigChange({ ...config, direction: event.currentTarget.value as "above" | "below" })} disabled={locked}>
        <option value="above">Above</option><option value="below">Below</option>
      </select>
    </label>
    <label>Threshold{selectedSignal ? ` (${selectedSignal.unit})` : ""}
      <input class="text-input" type="number" step="any" value={config.threshold} oninput={(event) => updateNumber("threshold", event.currentTarget.value)} disabled={locked} />
    </label>
    <label>Hysteresis{selectedSignal ? ` (${selectedSignal.unit})` : ""}
      <input class="text-input" type="number" min="0" step="any" value={config.hysteresis} oninput={(event) => updateNumber("hysteresis", event.currentTarget.value)} disabled={locked} />
    </label>
    <label>Hold (ms)
      <input class="text-input" type="number" min="0" max="60000" step="100" value={config.hold_ms} oninput={(event) => updateNumber("hold_ms", event.currentTarget.value)} disabled={locked} />
    </label>
    <label>Pre-trigger (ms)
      <input class="text-input" type="number" min="0" max="30000" step="500" value={config.pre_trigger_ms} oninput={(event) => updateNumber("pre_trigger_ms", event.currentTarget.value)} disabled={locked} />
    </label>
  </div>
  <div class="trigger-actions">
    {#if status.armed}
      <button class="button secondary" type="button" onclick={onDisarm} disabled={disabled || status.recording}>Disarm</button>
    {:else}
      <button class="button secondary" type="button" onclick={onArm} disabled={disabled || status.recording || signals.length === 0}>Arm trigger</button>
    {/if}
    <small title={status.recording_path ?? undefined}>{status.recording_path ? "Recording to session file" : "One shot · recording continues until stopped"}</small>
  </div>
</details>
