import { invoke } from "@tauri-apps/api/core";

export type SubaruCatalogRow = {
  id: string;
  definition_id: string;
  source_id: string;
  source_display_name: string;
  source_revision: string;
  catalog_sha256: string;
  source_table: string;
  source_did: string;
  ecu_key: string;
  ecu_id: string;
  ecu_name: string;
  protocol: string;
  data_rate: number | null;
  request_header: string;
  response_header: string;
  n_ta: string | null;
  request_hint: string;
  request_service: string;
  request_identifier: string;
  mapping_state: string;
  name: string;
  unit: string;
  vehicle_scope_json: string;
  vehicle_validation: string;
  auto_request: boolean;
  can_manual_read: boolean;
  manual_read_reason: string;
  decoder_supported: boolean;
  can_graph: boolean;
  graph_reason: string;
};

export type SubaruCatalogDetail = {
  row: SubaruCatalogRow;
  source_did: string;
  signal_id: string;
  source_pid_id: string;
  decoder_json: string;
  raw_source_json: string;
  link_metadata_json: string;
};

export type SubaruCatalogPage = {
  total: number;
  offset: number;
  limit: number;
  rows: SubaruCatalogRow[];
};

export type SubaruCatalogFilter = {
  query: string;
  model: string;
  year: number | null;
  ecuId: string;
  protocolFamily: string;
  offset: number;
  limit: number;
};

export function formatSubaruVehicleScope(value: string): string {
  const labels = subaruVehicleScopeLabels(value);
  if (!labels) return value || "Vehicle scope unresolved";
  return labels.length ? labels.join(" · ") : "No exact vehicle mapping";
}

export function previewSubaruVehicleScope(value: string): { text: string; remaining: number } {
  const labels = subaruVehicleScopeLabels(value);
  if (!labels) return { text: value || "Vehicle scope unresolved", remaining: 0 };
  if (!labels.length) return { text: "No exact vehicle mapping", remaining: 0 };
  return {
    text: labels.slice(0, 3).join(" · "),
    remaining: Math.max(0, labels.length - 3),
  };
}

function subaruVehicleScopeLabels(value: string): string[] | null {
  try {
    const scope = JSON.parse(value) as unknown;
    if (!Array.isArray(scope)) return null;
    return scope.flatMap((item) => {
      if (typeof item === "string") return item ? [item] : [];
      if (!item || typeof item !== "object") return [];
      const row = item as { display_label?: string; is_pseudo?: number };
      return row.is_pseudo || !row.display_label ? [] : [row.display_label];
    });
  } catch {
    return null;
  }
}

export function searchSubaruCatalog(filter: SubaruCatalogFilter): Promise<SubaruCatalogPage> {
  return invoke<SubaruCatalogPage>("search_subaru_catalog", filter);
}

export function getSubaruCatalogDetail(candidateId: string): Promise<SubaruCatalogDetail> {
  return invoke<SubaruCatalogDetail>("get_subaru_catalog_detail", { candidateId });
}
