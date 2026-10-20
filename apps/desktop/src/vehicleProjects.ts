import { invoke } from "@tauri-apps/api/core";
import type { GraphPreferences } from "./graphPreferences";
import type { ProfileSelectionSnapshot, VehicleProfile } from "./vehicleProfiles";
import type { WorkspacePreferences } from "./workspace";

export type VehicleProjectWorkspace = {
  version: 1;
  graph_preferences: GraphPreferences | null;
  layout_preferences: Partial<WorkspacePreferences> | null;
  catalog_filters: Record<string, unknown>;
  current_preset_id: string | null;
};
export type VehicleProject = {
  profile: VehicleProfile;
  workspace: VehicleProjectWorkspace;
};
export type VehicleProjectStatus = {
  state: "pending_choice" | "ready" | "unknown_vin" | "needs_prepare";
  connection_generation: number;
  project_id: string | null;
  project: VehicleProject | null;
  choice_required: boolean;
  reason?: string;
};
export type VehicleProjectsResult =
  | { kind: "status"; state: VehicleProjectStatus["state"]; connection_generation: number; project_id: string | null; project: VehicleProject | null; choice_required: boolean }
  | { kind: "projects"; projects: VehicleProject[] }
  | { kind: "project"; project: VehicleProject | null }
  | { kind: "prepared"; state: "created" | "pending_choice" | "unknown_vin"; project: VehicleProject | null; choice_required: boolean; reason: string | null }
  | { kind: "loaded"; project: VehicleProject; eligibility: unknown; applied: { selection: ProfileSelectionSnapshot; skipped: { reference: string }[] } }
  | { kind: "fresh"; state: "fresh"; project: VehicleProject; selection: ProfileSelectionSnapshot }
  | { kind: "manual_selected"; project: VehicleProject; selection: ProfileSelectionSnapshot; skipped_refs: string[] }
  | { kind: "saved"; project: VehicleProject };
export type VehicleProjectUpdatedEvent = { connection_generation: number; result: VehicleProjectsResult };
export type VehicleProjectOperation =
  | { op: "status" | "list" }
  | { op: "get"; project_id: string }
  | { op: "prepare_on_connect"; expected_generation: number }
  | { op: "load_last_state" | "start_fresh"; project_id: string; expected_revision: number; expected_generation: number }
  | { op: "select_manual"; project_id: string; expected_generation: number }
  | { op: "save"; project_id: string; expected_revision: number; expected_generation: number; selection: ProfileSelectionSnapshot; workspace: VehicleProjectWorkspace };

export function vehicleProjects(operation: VehicleProjectOperation): Promise<unknown> {
  return invoke("vehicle_projects", { operation });
}

export function emptyVehicleProjectWorkspace(): VehicleProjectWorkspace {
  return { version: 1, graph_preferences: null, layout_preferences: null, catalog_filters: {}, current_preset_id: null };
}

export function normalizeVehicleProjectWorkspace(value: unknown): VehicleProjectWorkspace {
  if (!value || typeof value !== "object" || Array.isArray(value)) return emptyVehicleProjectWorkspace();
  const raw = value as Record<string, unknown>;
  const filters = raw.catalog_filters && typeof raw.catalog_filters === "object" && !Array.isArray(raw.catalog_filters)
    ? raw.catalog_filters as Record<string, unknown> : {};
  return {
    version: 1,
    graph_preferences: raw.graph_preferences && typeof raw.graph_preferences === "object" ? raw.graph_preferences as GraphPreferences : null,
    layout_preferences: raw.layout_preferences && typeof raw.layout_preferences === "object" ? raw.layout_preferences as Partial<WorkspacePreferences> : null,
    catalog_filters: filters,
    current_preset_id: typeof raw.current_preset_id === "string" ? raw.current_preset_id : null,
  };
}

export function vehicleProjectName(profile: VehicleProfile): string {
  return profile.masked_vin ? `Vehicle · ${profile.masked_vin}` : profile.name;
}
