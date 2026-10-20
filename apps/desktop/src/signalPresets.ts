import type { ProfileSelectionSnapshot } from "./vehicleProfiles";
import { invoke } from "@tauri-apps/api/core";

export type PresetApplyMode = "replace" | "add";
export type PresetScope = "generic" | "model_powertrain" | "exact_profile";
export type SignalPreset = {
  id: string; revision: number; name: string; family: string; size: "core" | "extended";
  origin: "builtin" | "custom"; scope: PresetScope;
  criteria: { make: string | null; model: string | null; year: number | null; engine: string | null; transmission: string | null; profile_id: string | null };
  channels: { concept: string; signal_id: string | null; task_defining: boolean; priority: number; slow_context: boolean; oem_alternatives: unknown[] }[];
  selection: ProfileSelectionSnapshot | null;
};
export type PresetContext = { make: string | null; model: string | null; year: number | null; engine: string | null; transmission: string | null; profile_id: string | null; revision: number; connection_generation: number | null };
export type PresetAvailability = { concept: string; signal_id: string | null; eligible: boolean; reason: string | null; task_defining: boolean; priority: number; slow_context: boolean };
export type PresetPreview = { preview_id: string; preset: SignalPreset; mode: PresetApplyMode; eligible: PresetAvailability[]; skipped: PresetAvailability[]; task_defining_eligible: number; task_defining_total: number; connection_generation: number | null; can_apply: boolean; local_only?: boolean };

export type SignalPresetsOperation =
  | { op: "list" } | { op: "get"; id: string } | { op: "suggest"; context: PresetContext }
  | { op: "set_context"; context: PresetContext; expected_revision: number }
  | { op: "stage"; preset_id: string; preset_revision: number; mode: PresetApplyMode; expected_revision: number }
  | { op: "preview"; preset_id: string; mode: PresetApplyMode; expected_context_revision: number; expected_generation: number }
  | { op: "apply"; preview_id: string; expected_context_revision: number; expected_generation: number } | { op: "save"; preset: SignalPreset; expected_revision: number | null }
  | { op: "delete"; id: string; expected_revision: number } | { op: "status" } | { op: "cancel"; preview_id?: string };

export function signalPresets(operation: SignalPresetsOperation) {
  return invoke("signal_presets", { operation });
}

export async function stagePreset(
  presetId: string,
  presetRevision: number,
  mode: PresetApplyMode,
  context: PresetContext,
  request: (operation: SignalPresetsOperation) => Promise<unknown> = signalPresets,
): Promise<PresetContext> {
  const result = await request({
    op: "stage",
    preset_id: presetId,
    preset_revision: presetRevision,
    mode,
    expected_revision: context.revision,
  }) as { type: "picker_state"; state: { context: PresetContext } };
  return result.state.context;
}

export function acceptLatestPreview<T>(currentRequest: number, responseRequest: number, response: T): T | null {
  return currentRequest === responseRequest ? response : null;
}

export function presetPreviewMatchesConnection(preview: PresetPreview, connected: boolean, generation: number): boolean {
  return preview.connection_generation === null ? !connected : connected && preview.connection_generation === generation;
}

export function makeOfflinePresetPreview(preset: SignalPreset, mode: PresetApplyMode): PresetPreview {
  return {
    preview_id: "",
    preset,
    mode,
    eligible: [],
    skipped: preset.channels.map((channel) => ({
      concept: channel.concept,
      signal_id: channel.signal_id,
      eligible: false,
      reason: "connect_to_check_support",
      task_defining: channel.task_defining,
      priority: channel.priority,
      slow_context: channel.slow_context,
    })),
    task_defining_eligible: 0,
    task_defining_total: preset.channels.filter((channel) => channel.task_defining).length,
    connection_generation: null,
    can_apply: false,
    local_only: true,
  };
}

export function presetFromSelection(name: string, scope: PresetScope, selection: ProfileSelectionSnapshot, definitions: { id: string; name: string }[]): SignalPreset {
  const labels = new Map(definitions.map(({ id, name: label }) => [id, label]));
  const ids = [...new Set([...selection.log_signal_ids, ...selection.graph_signal_ids])];
  const id = `custom-${globalThis.crypto?.randomUUID?.() ?? `${Date.now()}-${Math.random().toString(16).slice(2)}`}`;
  return { id, revision: 0, name, family: "engine_tuning", size: "core", origin: "custom", scope,
    criteria: { make: null, model: null, year: null, engine: null, transmission: null, profile_id: null },
    channels: ids.map((signal_id) => {
      const candidateId = signal_id.startsWith("oem:") ? signal_id.slice(4) : null;
      const source = candidateId && selection.oem_references.find((reference) => reference.candidate_id === candidateId || reference.candidate_id === signal_id);
      return {
        concept: labels.get(signal_id) ?? signal_id,
        signal_id: candidateId ? null : signal_id,
        task_defining: false,
        priority: 0,
        slow_context: false,
        oem_alternatives: source ? [structuredClone(source)] : [],
      };
    }),
    selection: structuredClone(selection) };
}

export function presetCriteriaForScope(scope: PresetScope, context: PresetContext): SignalPreset["criteria"] {
  if (scope === "generic") {
    return { make: null, model: null, year: null, engine: null, transmission: null, profile_id: null };
  }
  return {
    make: context.make,
    model: context.model,
    year: context.year,
    engine: context.engine,
    transmission: context.transmission,
    profile_id: scope === "exact_profile" ? context.profile_id : null,
  };
}

export function presetScopeContextError(scope: PresetScope, context: PresetContext): string | null {
  if (scope === "model_powertrain" && (!context.make?.trim() || !context.model?.trim())) {
    return "Enter a make and model before using Model / powertrain scope.";
  }
  if (scope === "exact_profile" && !context.profile_id?.trim()) {
    return "Choose a saved vehicle profile before using Exact saved profile scope.";
  }
  return null;
}

export function updateCustomPresetMetadata(
  preset: SignalPreset,
  name: string,
  family: string,
  scope: PresetScope,
  context: PresetContext,
): SignalPreset {
  return {
    ...structuredClone(preset),
    name: name.trim(),
    family,
    scope,
    criteria: scope === preset.scope ? structuredClone(preset.criteria) : presetCriteriaForScope(scope, context),
  };
}

export async function latestPreview<T>(request: (token: number) => Promise<T>, state: { token: number }): Promise<T | null> {
  const token = ++state.token;
  const result = await request(token);
  return acceptLatestPreview(state.token, token, result);
}
