import type { CanIdReplayStatus } from "./types";

export type SavedCanReplaySelection = {
  can_id: number;
  extended: boolean;
  start_elapsed_ns: number;
  end_elapsed_ns: number;
};

export type SavedCanReplaySourceRef = { source_id: string; name: string; size_bytes: number };

export type SavedCanReplaySummary = SavedCanReplaySelection & {
  source_sha256: string;
  first_row: number;
  last_row: number;
  first_elapsed_ns: number;
  last_elapsed_ns: number;
  matching_rx_count: number;
  excluded_tx_count: number;
  excluded_flagged_count: number;
  source_span_ns: number;
  paced_duration_ns: number;
  source_bitrate: number | null;
};

export type SavedCanReplayTargetView = {
  generation: number;
  connected: boolean;
  source_id: string | null;
  channel: number | null;
  bitrate: number | null;
  listen_only: boolean | null;
  output_capture_active: boolean;
  output_capture_id: number | null;
  can_start: boolean;
  start_block_reason: string | null;
};

export type SavedCanReplayPreviewView = {
  preview_id: string;
  source_id: string;
  source_name: string;
  summary: SavedCanReplaySummary;
  source_bitrate: number | null;
  expires_at_unix_ms: number;
  target: SavedCanReplayTargetView;
};

export function sameReviewedCanTarget(
  reviewed: SavedCanReplayTargetView,
  current: SavedCanReplayTargetView,
): boolean {
  return reviewed.generation === current.generation
    && reviewed.source_id === current.source_id
    && reviewed.channel === current.channel
    && reviewed.bitrate === current.bitrate
    && reviewed.listen_only === current.listen_only
    && reviewed.output_capture_id !== null
    && reviewed.output_capture_id === current.output_capture_id;
}

export type SavedCanReplayInvoke = <T>(command: string, args?: Record<string, unknown>) => Promise<T>;

const NS_PER_MS = 1_000_000;

export function parseSavedCanReplayRangeMs(
  startText: string,
  endText: string,
): { start_elapsed_ns: number; end_elapsed_ns: number } | { error: string } {
  type ParsedMilliseconds = { kind: "value"; nanoseconds: number } | { kind: "error"; error: string };
  const parse = (text: string): ParsedMilliseconds => {
    const value = text.trim();
    if (/^-[\d]+$/.test(value)) return { kind: "error", error: "Replay times must be non-negative." };
    if (!/^\d+$/.test(value)) return { kind: "error", error: "Enter whole milliseconds for both replay times." };
    const milliseconds = Number(value);
    if (!Number.isSafeInteger(milliseconds) || milliseconds > Number.MAX_SAFE_INTEGER / NS_PER_MS) {
      return { kind: "error", error: "Replay time is too large to convert safely to nanoseconds." };
    }
    return { kind: "value", nanoseconds: milliseconds * NS_PER_MS };
  };
  const start = parse(startText);
  if (start.kind === "error") return { error: start.error };
  const end = parse(endText);
  if (end.kind === "error") return { error: end.error };
  if (end.nanoseconds < start.nanoseconds) {
    return { error: "Replay range start must not exceed its end." };
  }
  return { start_elapsed_ns: start.nanoseconds, end_elapsed_ns: end.nanoseconds };
}

export function formatSavedCanId(canId: number, extended: boolean): string {
  return `${extended ? "29-bit" : "11-bit"} 0x${canId.toString(16).toUpperCase().padStart(extended ? 8 : 3, "0")}`;
}

export function formatSavedCanReplaySpan(durationNs: number): string {
  return `${(Math.max(0, durationNs) / 1_000_000_000).toFixed(3)} s`;
}

export function createSavedCanReplayCommands(invoke: SavedCanReplayInvoke) {
  return {
    registerSource: (path: string) =>
      invoke<SavedCanReplaySourceRef>("register_saved_can_replay_source", { path }),
    listSources: () => invoke<SavedCanReplaySourceRef[]>("list_saved_can_replay_sources"),
    prepare: (sourceId: string, selection: SavedCanReplaySelection) =>
      invoke<SavedCanReplayPreviewView>("prepare_saved_can_replay", { sourceId, selection }),
    inspect: (previewId: string) =>
      invoke<SavedCanReplayPreviewView>("get_saved_can_replay_preview", { previewId }),
    discard: (previewId?: string) =>
      invoke<void>("discard_saved_can_replay_preview", { previewId: previewId ?? null }),
    start: (previewId: string, expectedCanGeneration: number, expectedOutputCaptureId: number) =>
      invoke<CanIdReplayStatus>("start_saved_can_replay", { previewId, expectedCanGeneration, expectedOutputCaptureId }),
    stop: () => invoke<void>("stop_can_id_replay"),
  };
}
