import type { CanLiveEventGroup } from "./types";

export type CanEventSnapshotCell =
  | { state: "pending" | "incomplete" | "absent" | "unknown" }
  | {
      state: "observed";
      frame_count: number;
      payload: number[];
      relation: "exact" | "similar" | "different";
      bit_match_percent?: number;
    };

export type CanEventSnapshotRow = {
  can_id: number;
  extended: boolean;
  known_observations: number;
  exact_recurrence_percent: number;
  total_frames: number;
  cells: CanEventSnapshotCell[];
};

export type CanEventSnapshotComparison = {
  rows: CanEventSnapshotRow[];
  matchingRows: number;
  totalRows: number;
};

type IdSummary = {
  can_id: number;
  extended: boolean;
  modes: Map<string, { payload: number[]; count: number; firstSequence: number }>;
  cappedRetainedWindows: number;
  totalFrames: number;
};

function payloadKey(bytes: number[]): string {
  return bytes.join(",");
}

function idKey(can_id: number, extended: boolean): string {
  return `${extended}:${can_id}`;
}

function matchesId(can_id: number, extended: boolean, query: string): boolean {
  const normalized = query.trim().toLowerCase();
  if (!normalized) return true;
  const hex = can_id.toString(16).toLowerCase();
  return `0x${hex.padStart(extended ? 8 : 3, "0")}`.includes(normalized)
    || `0x${hex}`.includes(normalized)
    || (extended ? "29-bit" : "11-bit").includes(normalized);
}

function bitMatch(left: number[], right: number[]): { matches: number; bits: number } {
  let matches = 0;
  for (let i = 0; i < left.length; i++) {
    let differing = left[i] ^ right[i];
    for (; differing; differing &= differing - 1) matches++;
  }
  const bits = left.length * 8;
  return { matches: bits - matches, bits };
}

export function compareCanEventSnapshots(
  group: CanLiveEventGroup,
  idFilter = "",
  rowLimit = 100,
): CanEventSnapshotComparison {
  const windows = [...group.windows].sort((a, b) => a.sequence - b.sequence);
  const summaries = new Map<string, IdSummary>();
  let completeWindows = 0;
  let cappedWindows = 0;

  for (const window of windows) {
    if (window.state !== "complete") continue;
    completeWindows++;
    if (window.omitted_ids > 0) cappedWindows++;
    for (const message of window.messages) {
      const key = idKey(message.can_id, message.extended);
      let summary = summaries.get(key);
      if (!summary) {
        summary = {
          can_id: message.can_id,
          extended: message.extended,
          modes: new Map(),
          cappedRetainedWindows: 0,
          totalFrames: 0,
        };
        summaries.set(key, summary);
      }
      if (window.omitted_ids > 0) summary.cappedRetainedWindows++;
      summary.totalFrames += message.frame_count;
      const payload = message.latest_frame.data;
      const payloadId = payloadKey(payload);
      const mode = summary.modes.get(payloadId);
      if (mode) mode.count++;
      else summary.modes.set(payloadId, { payload, count: 1, firstSequence: window.sequence });
    }
  }

  const candidates = [...summaries.values()].map((summary) => {
    const mode = [...summary.modes.values()].sort((a, b) => b.count - a.count || a.firstSequence - b.firstSequence)[0];
    const knownObservations = completeWindows - (cappedWindows - summary.cappedRetainedWindows);
    return { ...summary, mode, knownObservations, exactRecurrencePercent: knownObservations ? (mode?.count ?? 0) * 100 / knownObservations : 0 };
  });
  const matching = candidates.filter((row) => matchesId(row.can_id, row.extended, idFilter));
  matching.sort((a, b) => b.exactRecurrencePercent - a.exactRecurrencePercent
    || b.totalFrames - a.totalFrames
    || Number(a.extended) - Number(b.extended)
    || a.can_id - b.can_id);

  const messageMaps = windows.map((window) => new Map(window.messages.map((message) => [idKey(message.can_id, message.extended), message])));
  const rows = matching.slice(0, Math.max(0, rowLimit)).map((summary) => {
    const cells: CanEventSnapshotCell[] = windows.map((window, index) => {
      if (window.state === "pending") return { state: "pending" };
      if (window.state === "incomplete") return { state: "incomplete" };
      const message = messageMaps[index].get(idKey(summary.can_id, summary.extended));
      if (!message) return { state: window.omitted_ids > 0 ? "unknown" : "absent" };
      const payload = message.latest_frame.data;
      if (!summary.mode || payloadKey(payload) === payloadKey(summary.mode.payload)) {
        return { state: "observed", frame_count: message.frame_count, payload, relation: "exact" };
      }
      if (payload.length !== summary.mode.payload.length) {
        return { state: "observed", frame_count: message.frame_count, payload, relation: "different" };
      }
      const { matches, bits } = bitMatch(payload, summary.mode.payload);
      return {
        state: "observed",
        frame_count: message.frame_count,
        payload,
        relation: bits > 0 && matches * 10 >= bits * 9 ? "similar" : "different",
        ...(bits > 0 ? { bit_match_percent: matches * 100 / bits } : {}),
      };
    });
    return {
      can_id: summary.can_id,
      extended: summary.extended,
      known_observations: summary.knownObservations,
      exact_recurrence_percent: summary.exactRecurrencePercent,
      total_frames: summary.totalFrames,
      cells,
    };
  });

  return { rows, matchingRows: matching.length, totalRows: summaries.size };
}
