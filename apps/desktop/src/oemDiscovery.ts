import type { OemCatalogPage, OemCandidateTryResult } from "./types";

export type OemDiscoveryState = "queued" | "running" | "value" | "reply" | "no_response" | "negative" | "error" | "unsupported";

export type OemDiscoveryResult = {
  candidate_id: string;
  state: OemDiscoveryState;
  outcome: string | null;
  value: number | null;
  unit: string;
  detail: string | null;
  source_blob_sha: string;
  evidence_saved: boolean;
};

export type OemDiscoveryFilter = {
  query: string;
  brand: string;
  model: string;
  service: string | null;
};

export type OemDiscoveryStatus = {
  adapter_id: string | null;
  connection_generation: number | null;
  auto_try_visible: boolean;
  running: boolean;
  queued: number;
  completed: number;
  with_values: number;
  current_candidate_id: string | null;
  last_error: string | null;
  revision: number;
};

export type OemDiscoveryResponse = {
  status: OemDiscoveryStatus;
  page?: OemCatalogPage | null;
  results: OemDiscoveryResult[];
};

/** Present an explicit one-shot result alongside background discovery evidence. */
export function oemReadDiscoveryResult(result: OemCandidateTryResult, sourceBlobSha: string): OemDiscoveryResult {
  const numeric = result.outcome === "positive" && Number.isFinite(result.decoded_value?.value);
  const state: OemDiscoveryState = numeric ? "value"
    : result.outcome === "positive" ? "reply"
      : result.outcome === "no_response" ? "no_response"
        : ["negative_response", "response_pending", "temporarily_unavailable", "session_required", "security_required"].includes(result.outcome) ? "negative" : "error";
  return {
    candidate_id: result.candidate_id,
    state,
    outcome: result.outcome,
    value: numeric ? result.decoded_value!.value : null,
    unit: result.decoded_value?.unit ?? "",
    detail: result.error ?? result.decode_error ?? result.decoded_value?.enum_value ?? null,
    source_blob_sha: sourceBlobSha,
    evidence_saved: result.evidence_saved,
  };
}

export function oemDiscoveryFilter(query: string, brand: string, model: string, service: string): OemDiscoveryFilter {
  return {
    query: query.trim(),
    brand: brand.trim(),
    model: model.trim(),
    service: service || null,
  };
}

export function uniqueCandidateIds(ids: readonly string[]): string[] {
  return [...new Set(ids.filter(Boolean))];
}

export function nextVisibleOemCandidateIds(ids: readonly string[], attempted: ReadonlySet<string>): string[] {
  return uniqueCandidateIds(ids).filter((id) => !attempted.has(id));
}

export function updateVisibleOemCandidateIds(ids: readonly string[], candidateId: string, visible: boolean): string[] {
  const next = new Set(ids);
  if (visible) next.add(candidateId);
  else next.delete(candidateId);
  return [...next];
}

export function isCurrentOemDiscoveryConnection(
  status: Pick<OemDiscoveryStatus, "adapter_id" | "connection_generation">,
  adapterId: string | null,
  connectionGeneration: number | null,
): boolean {
  return Boolean(adapterId && connectionGeneration !== null
    && status.adapter_id === adapterId
    && status.connection_generation === connectionGeneration);
}

export function sortOemDiscoveryResults<T extends Pick<OemDiscoveryResult, "state" | "value">>(results: readonly T[]): T[] {
  return results
    .map((result, index) => ({ result, index }))
    .sort((left, right) => {
      const a = left.result.state === "value" && Number.isFinite(left.result.value);
      const b = right.result.state === "value" && Number.isFinite(right.result.value);
      if (a !== b) return a ? -1 : 1;
      return left.index - right.index;
    })
    .map(({ result }) => result);
}

export function oemDiscoveryPresentation(result: Pick<OemDiscoveryResult, "state" | "value" | "unit"> | null): { label: string; tone: "untested" | "waiting" | "active" | "value" | "reply" | "failure" } {
  if (!result) return { label: "Untested", tone: "untested" };
  if (result.state === "value" && Number.isFinite(result.value)) {
    return { label: `${result.value} ${result.unit}`.trim(), tone: "value" };
  }
  switch (result.state) {
    case "queued": return { label: "Queued", tone: "waiting" };
    case "running": return { label: "Running", tone: "active" };
    case "reply": return { label: "Reply · no numeric decode", tone: "reply" };
    case "negative": return { label: "Negative response", tone: "failure" };
    case "no_response": return { label: "No response", tone: "failure" };
    case "unsupported": return { label: "Unsupported by adapter", tone: "failure" };
    case "error": return { label: "Read error", tone: "failure" };
    default: return { label: "Untested", tone: "untested" };
  }
}
