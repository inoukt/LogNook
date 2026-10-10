import type { CanLiveEventGroup, CanLiveEventWindowSnapshot } from "./types";

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
  cells: CanEventSnapshotCell[];
};

function payloadKey(bytes: number[]): string {
  return bytes.join(",");
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

export function compareCanEventSnapshots(group: CanLiveEventGroup): CanEventSnapshotRow[] {
  const windows = [...group.windows].sort((a, b) => a.sequence - b.sequence);
  const ids = new Map<string, { can_id: number; extended: boolean }>();
  for (const window of windows) {
    if (window.state !== "complete") continue;
    for (const message of window.messages) {
      ids.set(`${message.extended}:${message.can_id}`, { can_id: message.can_id, extended: message.extended });
    }
  }

  return [...ids.values()]
    .sort((a, b) => Number(a.extended) - Number(b.extended) || a.can_id - b.can_id)
    .map(({ can_id, extended }) => {
      const observations = windows.flatMap((window) => {
        if (window.state !== "complete") return [];
        const message = window.messages.find((entry) => entry.can_id === can_id && entry.extended === extended);
        return message ? [{ window, payload: message.latest_frame.data }] : [];
      });
      const modes = new Map<string, { payload: number[]; count: number; firstSequence: number }>();
      for (const { window, payload } of observations) {
        const key = payloadKey(payload);
        const mode = modes.get(key);
        if (mode) mode.count++;
        else modes.set(key, { payload, count: 1, firstSequence: window.sequence });
      }
      const mode = [...modes.values()].sort((a, b) => b.count - a.count || a.firstSequence - b.firstSequence)[0];
      const knownObservations = windows.filter((window) => isKnown(window, can_id, extended)).length;
      const exactCount = mode ? mode.count : 0;
      const cells: CanEventSnapshotCell[] = windows.map((window) => {
        if (window.state === "pending") return { state: "pending" };
        if (window.state === "incomplete") return { state: "incomplete" };
        const message = window.messages.find((entry) => entry.can_id === can_id && entry.extended === extended);
        if (!message) return { state: window.omitted_ids > 0 ? "unknown" : "absent" };
        const payload = message.latest_frame.data;
        if (!mode || payloadKey(payload) === payloadKey(mode.payload)) {
          return { state: "observed", frame_count: message.frame_count, payload, relation: "exact" };
        }
        if (payload.length !== mode.payload.length) {
          return { state: "observed", frame_count: message.frame_count, payload, relation: "different" };
        }
        const { matches, bits } = bitMatch(payload, mode.payload);
        return {
          state: "observed",
          frame_count: message.frame_count,
          payload,
          relation: bits > 0 && matches * 10 >= bits * 9 ? "similar" : "different",
          ...(bits > 0 ? { bit_match_percent: matches * 100 / bits } : {}),
        };
      });
      return {
        can_id,
        extended,
        known_observations: knownObservations,
        exact_recurrence_percent: knownObservations ? exactCount * 100 / knownObservations : 0,
        cells,
      };
    });
}

function isKnown(window: CanLiveEventWindowSnapshot, can_id: number, extended: boolean): boolean {
  if (window.state !== "complete") return false;
  return window.omitted_ids === 0 || window.messages.some((message) => message.can_id === can_id && message.extended === extended);
}
