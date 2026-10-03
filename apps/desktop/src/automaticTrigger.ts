export type AutomaticTriggerConfig = {
  signal_id: string;
  direction: "above" | "below";
  threshold: number;
  hysteresis: number;
  hold_ms: number;
  pre_trigger_ms: number;
};

export type AutomaticTriggerStatus = {
  armed: boolean;
  recording: boolean;
  config: AutomaticTriggerConfig | null;
  recording_path: string | null;
};

export const defaultAutomaticTriggerConfig: AutomaticTriggerConfig = {
  signal_id: "engine_rpm",
  direction: "above",
  threshold: 3_000,
  hysteresis: 100,
  hold_ms: 0,
  pre_trigger_ms: 5_000,
};

export const disarmedAutomaticTriggerStatus: AutomaticTriggerStatus = {
  armed: false,
  recording: false,
  config: null,
  recording_path: null,
};

export function normalizeAutomaticTriggerConfig(
  value: Partial<AutomaticTriggerConfig>,
  acquiredSignalIds: string[],
  fallbackSignalId: string,
): AutomaticTriggerConfig {
  const allowed = new Set(acquiredSignalIds);
  const signalId = typeof value.signal_id === "string" && allowed.has(value.signal_id)
    ? value.signal_id
    : allowed.has(fallbackSignalId) ? fallbackSignalId : acquiredSignalIds[0] ?? fallbackSignalId;
  const finite = (candidate: unknown, fallback: number) => typeof candidate === "number" && Number.isFinite(candidate)
    ? candidate
    : fallback;
  return {
    signal_id: signalId,
    direction: value.direction === "below" ? "below" : "above",
    threshold: Math.max(-1_000_000_000, Math.min(1_000_000_000, finite(value.threshold, 3_000))),
    hysteresis: Math.max(0, Math.min(1_000_000_000, finite(value.hysteresis, 0))),
    hold_ms: Math.max(0, Math.min(60_000, Math.round(finite(value.hold_ms, 0)))),
    pre_trigger_ms: Math.max(0, Math.min(30_000, Math.round(finite(value.pre_trigger_ms, 0)))),
  };
}

export function restoreAutomaticTriggerConfig(
  serialized: string | null,
  acquiredSignalIds: string[],
  fallbackSignalId: string,
): AutomaticTriggerConfig {
  let saved: Partial<AutomaticTriggerConfig> = {};
  try {
    const value: unknown = JSON.parse(serialized ?? "null");
    if (value && typeof value === "object" && !Array.isArray(value)) {
      saved = value as Partial<AutomaticTriggerConfig>;
    }
  } catch { /* Ignore malformed optional preferences. */ }
  return normalizeAutomaticTriggerConfig(saved, acquiredSignalIds, fallbackSignalId);
}
