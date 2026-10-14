import { invoke } from "@tauri-apps/api/core";

export type ProfileScope = "live" | "simulator" | "can_only";
export type IdentityReadState = "positive" | "unsupported" | "no_response" | "malformed" | "transport_error" | "not_attempted";
export type ProfileMatchState = "exact" | "review_required" | "identity_unavailable" | "conflict";
export type VehicleIdentitySource = "report_ecu" | "same_generation_recognition" | "manual_profile" | "conflict" | "unassigned";
export type VinOrigin = "ecu_report" | "ecu_recognition" | "legacy_report_import" | "owner_entered" | "agent_entered";
export type ManualVehicleField = "make" | "model" | "model_year" | "engine" | "trim";
export type ManualDetailActor = "owner_ui" | "agent_access";
export type VinCheckDigitStatus = "match" | "mismatch_warning" | "unknown_scope";

export type WmiAssignment = {
  wmi_key: string;
  manufacturer: string;
  manufacturer_assignment_country: string | null;
  vehicle_type: string | null;
  class_scope: string | null;
  source_id: string;
  source_revision: string;
};

export type VinDecodeResult = {
  normalized_vin: string;
  check_digit_status: VinCheckDigitStatus;
  check_digit_scheme: string | null;
  year_code: string;
  candidate_model_years: number[];
  class_backed_model_year: number | null;
  wmi_assignment: WmiAssignment | null;
  decoder_revision: string;
  wmi_dataset_sha256: string;
};

export type ManualVehicleDetail = {
  field: ManualVehicleField;
  value: string;
  actor: ManualDetailActor;
  entered_at_unix_ms: number;
  note: string | null;
};

export type VinEvidenceSource = {
  origin: VinOrigin;
  recorded_at_unix_ms: number;
  report_id: string | null;
  ecu_id: string | null;
  connection_generation: number | null;
  actor: ManualDetailActor | null;
};

export type VehicleIdentityRecord = {
  scope: ProfileScope;
  vin_digest: string;
  full_vin: string | null;
  origin_evidence: VinEvidenceSource[];
  decoder_snapshot: VinDecodeResult | null;
  owner_details: ManualVehicleDetail[];
  revision: number;
};

export type VehicleIdentitySnapshot = {
  scope: ProfileScope;
  connection_generation: number | null;
  profile_id: string | null;
  profile_revision: number | null;
  full_vin: string | null;
  vin_digest: string | null;
  masked_vin: string | null;
  origin: VinOrigin | null;
  identity_source: VehicleIdentitySource;
  vin_conflict: boolean;
  observed_ecus: EcuProfileVariant[];
  decoded_snapshot: VinDecodeResult | null;
  owner_details: ManualVehicleDetail[];
};

export type EcuProfileVariant = {
  protocol: string;
  request_route: string;
  request_address: string | null;
  response_address: string;
  ecu_name: string | null;
  calibration_ids: string[];
  software_ids: string[];
  ecu_name_state: IdentityReadState;
  calibration_state: IdentityReadState;
  software_state: IdentityReadState;
};

export type VehicleIdentityObservation = {
  obd_connection_generation: number;
  observed_at_unix_ms: number;
  scope: ProfileScope;
  vin_digest: string | null;
  masked_vin: string | null;
  vin_status: IdentityReadState;
  conflicting_vin_observations: boolean;
  ecu_variants: EcuProfileVariant[];
  identity_supported: boolean;
};

export type DerivedSignalDefinition = {
  id: string;
  name: string;
  expression: string;
  unit: string;
  tare_offset: number;
};

export type CanSignalDefinition = {
  id: string;
  name: string;
  can_id: number;
  extended: boolean;
  start_bit: number;
  bit_length: number;
  byte_order: "intel" | "motorola";
  signed: boolean;
  factor: number;
  offset: number;
  unit: string;
};

export type OemDefinitionReference = {
  candidate_id: string;
  source_id: string;
  source_revision: string;
  decoder_revision: string;
};

export type ProfileSelectionSnapshot = {
  log_signal_ids: string[];
  graph_signal_ids: string[];
  derived_definitions: DerivedSignalDefinition[];
  can_fields: CanSignalDefinition[];
  can_source_id: string | null;
  can_bitrate: number | null;
  can_channel: number | null;
  oem_references: OemDefinitionReference[];
};

export type VehicleProfile = {
  id: string;
  name: string;
  revision: number;
  scope: ProfileScope;
  catalog_vehicle_id: string | null;
  vin_digest: string | null;
  masked_vin: string | null;
  manual_association: boolean;
  ecu_variants: EcuProfileVariant[];
  selection: ProfileSelectionSnapshot;
  observations: VehicleIdentityObservation[];
  created_at_unix_ms: number;
  updated_at_unix_ms: number;
};

export type ProfileMatch = {
  state: ProfileMatchState;
  fingerprint_digest: string | null;
  matched_ecu_variants: string[];
  review_reasons: string[];
};

export type ProfileResumeSkipReason =
  | "unsupported_signal"
  | "missing_dependency"
  | "oem_exact_catalog_validation_required"
  | "can_source_unavailable"
  | "unknown_reference"
  | "identity_conflict"
  | "reconnect_for_standby"
  | "stale_definition";

export type SkippedProfileSelection = {
  reference: string;
  surface: "log" | "graph" | "can";
  reason: ProfileResumeSkipReason;
  detail: string;
};

export type ProfileResumePreview = {
  preview_id: string;
  profile_id: string;
  profile_revision: number;
  obd_connection_generation: number;
  profile_match: ProfileMatch;
  eligible_log_refs: string[];
  eligible_graph_refs: string[];
  skipped: SkippedProfileSelection[];
  can_apply: boolean;
  manual_association_available: boolean;
};

export type ProfileResumeApplyResult = {
  preview_id: string;
  profile_id: string;
  profile_revision: number;
  obd_connection_generation: number;
  applied_log_refs: string[];
  eligible_graph_refs: string[];
  skipped: SkippedProfileSelection[];
  selection: ProfileSelectionSnapshot;
  graph_options: ProfileGraphOption[];
};

export type ProfileGraphOption = {
  id: string;
  name: string;
  unit: string;
  description: string;
  research: boolean;
};

export type VehicleProfileMatchSummary = {
  profile_id: string;
  name: string;
  revision: number;
  profile_match: ProfileMatch;
};

export type VehicleProfileStatus = {
  obd_connected: boolean;
  profile_standby: boolean;
  obd_connection_generation: number;
  current_observation: VehicleIdentityObservation | null;
  profile_matches: VehicleProfileMatchSummary[];
  active_profile_id: string | null;
  active_profile_revision: number | null;
  preview: ProfileResumePreview | null;
};

export type SaveVehicleProfileArgs = {
  profile_id: string | null;
  expected_revision: number | null;
  name: string;
  selection: ProfileSelectionSnapshot;
  manual_association: boolean;
  catalog_vehicle_id: string | null;
};

export type OemVehicleProfile = {
  vehicle_id: string;
  brand: string;
  model: string;
  model_year: number | null;
  engine: string;
};

export function vehicleIdentityVinPresentation(
  record: Pick<VehicleIdentityRecord, "full_vin" | "vin_digest"> & { masked_vin?: string | null } | null,
): { value: string; full: boolean; label: string } {
  if (record?.full_vin) return { value: record.full_vin, full: true, label: "Full VIN" };
  if (record?.masked_vin) return { value: record.masked_vin, full: false, label: "Masked VIN · full VIN unavailable" };
  if (record?.vin_digest) return { value: "VIN unavailable", full: false, label: "Legacy VIN digest only" };
  return { value: "VIN unavailable", full: false, label: "VIN unavailable" };
}

export async function copyFullVin(vin: string, writeText: (value: string) => Promise<void>): Promise<boolean> {
  if (!vin) return false;
  try { await writeText(vin); return true; }
  catch { return false; }
}

export function vinDecodeLabels(decode: VinDecodeResult): { checkDigit: string; year: string; manufacturer: string } {
  const checkDigit = decode.check_digit_status === "match"
    ? `Check digit matched${decode.check_digit_scheme ? ` · ${decode.check_digit_scheme}` : ""}`
    : decode.check_digit_status === "mismatch_warning"
      ? `Check-digit mismatch warning${decode.check_digit_scheme ? ` · ${decode.check_digit_scheme}` : ""}`
      : "Check-digit scope unknown";
  const year = decode.class_backed_model_year !== null
    ? `${decode.class_backed_model_year} · source-backed class rule`
    : decode.candidate_model_years.length > 1
      ? `Year unresolved · ${decode.candidate_model_years.join(" or ")}`
      : decode.candidate_model_years.length === 1
        ? String(decode.candidate_model_years[0])
        : "Model year unavailable";
  return { checkDigit, year, manufacturer: decode.wmi_assignment?.manufacturer ?? "Unknown WMI" };
}

export function canApplyProfileResume(
  preview: ProfileResumePreview | null,
  connected: boolean,
  standby: boolean,
  currentGeneration: number,
): boolean {
  return Boolean(
    preview && connected && standby && preview.can_apply &&
    preview.profile_match.state !== "conflict" &&
    preview.obd_connection_generation === currentGeneration &&
    preview.eligible_log_refs.length > 0,
  );
}

export function createSaveVehicleProfileArgs(
  name: string,
  selection: ProfileSelectionSnapshot,
  manualAssociation: boolean,
  profileId: string | null = null,
  expectedRevision: number | null = null,
  catalogVehicleId: string | null = null,
): SaveVehicleProfileArgs {
  const cleaned = name.trim();
  if (!cleaned) throw new Error("Enter a name for this vehicle profile.");
  if (profileId !== null && expectedRevision === null) throw new Error("Updating a profile requires its current revision.");
  return {
    profile_id: profileId,
    expected_revision: expectedRevision,
    name: cleaned,
    selection,
    manual_association: manualAssociation,
    catalog_vehicle_id: catalogVehicleId,
  };
}

export const listVehicleProfiles = () => invoke<VehicleProfile[]>("list_vehicle_profiles");
export const getVehicleProfile = (profileId: string) => invoke<VehicleProfile>("get_vehicle_profile", { profileId });
export const saveVehicleProfile = (args: SaveVehicleProfileArgs) => invoke<VehicleProfile>("save_vehicle_profile", { args });
export const recognizeVehicle = (obdConnectionGeneration: number) =>
  invoke<VehicleIdentityObservation>("recognize_vehicle", { obdConnectionGeneration });
export const prepareProfileResume = (profileId: string, obdConnectionGeneration: number) =>
  invoke<ProfileResumePreview>("prepare_profile_resume", { profileId, obdConnectionGeneration });
export const applyProfileResume = (previewId: string, profileRevision: number, obdConnectionGeneration: number) =>
  invoke<ProfileResumeApplyResult>("apply_profile_resume", { previewId, profileRevision, obdConnectionGeneration });
export const discardProfileResume = (previewId: string) => invoke<void>("discard_profile_resume", { previewId });
export const vehicleProfilesStatus = () => invoke<VehicleProfileStatus>("vehicle_profiles_status");
export const getVehicleIdentity = (scope: ProfileScope, vinDigest: string) =>
  invoke<VehicleIdentityRecord | null>("get_vehicle_identity", { scope, vinDigest });
export const decodeVehicleVinOffline = (scope: ProfileScope, vinDigest: string) =>
  invoke<VehicleIdentityRecord>("decode_vehicle_vin_offline", { scope, vinDigest });
export const setManualVehicleDetail = (scope: ProfileScope, vinDigest: string, field: ManualVehicleField, value: string, note: string | null) =>
  invoke<VehicleIdentityRecord>("set_manual_vehicle_detail", { scope, vinDigest, field, value, note });
export const linkLegacyVehicleVin = (profileId: string, expectedRevision: number, fullVin: string) =>
  invoke<VehicleIdentityRecord>("link_legacy_vehicle_vin", { profileId, expectedRevision, fullVin });
export const assignManualProfileVin = (profileId: string, expectedRevision: number, fullVin: string) =>
  invoke<VehicleIdentityRecord>("assign_manual_profile_vin", { profileId, expectedRevision, fullVin });
