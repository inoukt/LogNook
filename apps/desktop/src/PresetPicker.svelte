<script lang="ts">
  import { tick } from "svelte";
  import { signalPresets, stagePreset, presetFromSelection, makeOfflinePresetPreview, presetCriteriaForScope, presetScopeContextError, updateCustomPresetMetadata, latestPreview, presetPreviewMatchesConnection, type SignalPreset, type PresetContext, type PresetPreview } from "./signalPresets";
  import type { ProfileSelectionSnapshot, VehicleProfile, VehicleProfileMatchSummary } from "./vehicleProfiles";
  import { getVehicleIdentity } from "./vehicleProfiles";
  let presets: SignalPreset[] = [];
  let context: PresetContext = { make: null, model: null, year: null, engine: null, transmission: null, profile_id: null, revision: 0, connection_generation: null };
  let tab: "suggested" | "all" | "my" = "suggested";
  let familyFilter = "";
  let query = "";
  let suggestedIds = new Set<string>();
  let preview: PresetPreview | null = null;
  let selected = "";
  let mode: "replace" | "add" = "replace";
  let name = "";
  let family = "engine_tuning";
  let editingPreset: SignalPreset | null = null;
  let previewElement: HTMLElement | undefined;
  let scope: "generic" | "model_powertrain" | "exact_profile" = "generic";
  let token = { token: 0 };
  let error = "";
  let prefillingProfile: string | null = null;
  $: if (preview && !presetPreviewMatchesConnection(preview, connected, generation)) {
    const stalePreviewId = preview.preview_id;
    const cancelServerPreview = !preview.local_only && !!stalePreviewId;
    preview = null;
    if (cancelServerPreview) void signalPresets({ op: "cancel", preview_id: stalePreviewId }).catch(() => {});
  }
  export let open = false;
  export let connected = false;
  export let generation = 0;
  export let logIds: string[] = [];
  export let graphIds: string[] = [];
  export let definitions: { id: string; name: string }[] = [];
  export let profiles: VehicleProfile[] = [];
  export let profileMatches: VehicleProfileMatchSummary[] = [];
  export let selectionSnapshot: ProfileSelectionSnapshot;
  export let selectedPresetId: string | null = null;
  export let onSelectedPresetId: (id: string | null) => void = () => {};
  export let onClose: () => void;
  export let onApply: (selection: ProfileSelectionSnapshot) => void;
  $: if (open) void refresh();
  $: if (open && selected !== (selectedPresetId ?? "")) selected = selectedPresetId ?? "";
  $: if (open && !context.profile_id) {
    const exactMatches = profileMatches.filter(({ profile_match }) => profile_match.state === "exact");
    if (exactMatches.length === 1) context = { ...context, profile_id: exactMatches[0].profile_id };
  }
  $: if (open && context.profile_id && prefillingProfile !== context.profile_id && profiles.some(({ id }) => id === context.profile_id)) {
    prefillingProfile = context.profile_id;
    void prefillProfileContext(context.profile_id);
  }
  async function refresh() {
    try {
      const result = await signalPresets({ op: "status" }) as { type: "status"; context: PresetContext; staged: { preset_id: string; preset_revision: number; mode: "replace" | "add" } | null };
      context = result.context;
      const response = await signalPresets({ op: "list" }) as { type: "presets"; presets: SignalPreset[] };
      presets = response.presets;
      const suggestions = await signalPresets({ op: "suggest", context }) as { type: "suggestions"; presets: SignalPreset[] };
      suggestedIds = new Set(suggestions.presets.map(({ id }) => id));
      if (!connected && result.staged) {
        const stagedPreset = presets.find((preset) => preset.id === result.staged?.preset_id && preset.revision === result.staged?.preset_revision);
        if (stagedPreset) {
          mode = result.staged.mode;
          preview = makeOfflinePresetPreview(stagedPreset, result.staged.mode);
          await tick();
          previewElement?.scrollIntoView({ block: "start" });
        }
      }
    } catch (e) { error = String(e); }
  }
  async function updateSuggestions() {
    try {
      const response = await signalPresets({ op: "suggest", context }) as { type: "suggestions"; presets: SignalPreset[] };
      suggestedIds = new Set(response.presets.map(({ id }) => id));
      tab = "suggested";
    } catch (e) { error = String(e); }
  }
  async function prefillProfileContext(profileId: string) {
    const profile = profiles.find(({ id }) => id === profileId);
    if (!profile) return;
    if (!profile.vin_digest) {
      context = { ...context, profile_id: profileId };
      await updateSuggestions();
      return;
    }
    try {
      const identity = await getVehicleIdentity(profile.scope, profile.vin_digest);
      if (context.profile_id !== profileId) return;
      const latest = (field: string) => [...(identity?.owner_details ?? [])].reverse().find((detail) => detail.field === field)?.value ?? null;
      const yearValue = latest("model_year");
      context = {
        ...context,
        profile_id: profileId,
        make: latest("make") ?? context.make,
        model: latest("model") ?? context.model,
        year: yearValue && /^\d{4}$/.test(yearValue) ? Number(yearValue) : context.year,
        engine: latest("engine") ?? context.engine,
        transmission: latest("transmission") ?? context.transmission,
      };
      await updateSuggestions();
    } catch (e) { error = String(e); }
  }
  $: visible = presets.filter((preset) => {
    if (tab === "my" && preset.origin !== "custom") return false;
    if (tab === "suggested" && !suggestedIds.has(preset.id)) return false;
    if (familyFilter && preset.family !== familyFilter) return false;
    const search = query.trim().toLocaleLowerCase();
    return !search || `${preset.name} ${preset.family} ${preset.scope}`.toLocaleLowerCase().includes(search);
  });
  async function makePreview(id: string) {
    selected = id; onSelectedPresetId(id); error = "";
    try {
      const preset = presets.find((item) => item.id === id);
      if (!preset) return;
      const savedContext = await signalPresets({ op: "set_context", context, expected_revision: context.revision }) as { type: "picker_state"; state: { context: PresetContext; revision: number } };
      context = savedContext.state.context;
      if (!connected) {
        context = await stagePreset(id, preset.revision, mode, context);
        preview = makeOfflinePresetPreview(preset, mode);
        await tick();
        previewElement?.scrollIntoView({ block: "start" });
        return;
      }
      const response = await latestPreview(async () => signalPresets({ op: "preview", preset_id: id, mode, expected_context_revision: context.revision, expected_generation: generation }) as Promise<{ type: "preview"; preview_id: string; mode: "replace" | "add"; can_apply: boolean; resolved: { eligible: PresetPreview["eligible"]; skipped: PresetPreview["skipped"]; task_defining_eligible: number; task_defining_total: number; connection_generation: number | null } }>, token);
      if (response) {
        preview = { preview_id: response.preview_id, mode: response.mode, can_apply: response.can_apply, preset, ...response.resolved };
        await tick();
        previewElement?.scrollIntoView({ block: "start" });
      }
    } catch (e) { error = String(e); }
  }
  async function apply() {
    if (!preview || !connected) return;
    try {
      const result = await signalPresets({ op: "apply", preview_id: preview.preview_id, expected_context_revision: context.revision, expected_generation: generation }) as { type: "applied"; resolved: { selection: ProfileSelectionSnapshot } };
      onApply(result.resolved.selection);
      preview = null;
    } catch (e) { error = String(e); }
  }
  async function clearStagedPreset() {
    try {
      const saved = await signalPresets({ op: "set_context", context, expected_revision: context.revision }) as { type: "picker_state"; state: { context: PresetContext } };
      context = saved.state.context;
      preview = null;
    } catch (e) { error = String(e); }
  }
  async function save() {
    const changingScope = !editingPreset || editingPreset.scope !== scope;
    if (changingScope) {
      const scopeError = presetScopeContextError(scope, context);
      if (scopeError) { error = scopeError; return; }
    }
    const selection: ProfileSelectionSnapshot = structuredClone(selectionSnapshot);
    selection.log_signal_ids = [...logIds];
    selection.graph_signal_ids = [...graphIds];
    const recipe = updateCustomPresetMetadata(editingPreset ?? presetFromSelection(name, scope, selection, definitions), name, family, scope, context);
    if (!editingPreset) {
      recipe.selection = selection;
      recipe.channels = presetFromSelection(recipe.name, scope, selection, definitions).channels;
    }
    try { await signalPresets({ op: "save", preset: recipe, expected_revision: editingPreset?.revision ?? null }); name = ""; editingPreset = null; await refresh(); tab = "my"; }
    catch (e) { error = String(e); }
  }
  async function copyPreset(preset: SignalPreset) {
    const copy = structuredClone(preset);
    copy.id = `custom-${globalThis.crypto?.randomUUID?.() ?? `${Date.now()}-${Math.random().toString(16).slice(2)}`}`;
    copy.revision = 0;
    copy.name = `${preset.name} copy`;
    copy.origin = "custom";
    copy.selection = null;
    if (context.profile_id) {
      copy.scope = "exact_profile";
      copy.criteria = presetCriteriaForScope(copy.scope, context);
    } else if (context.make && context.model) {
      copy.scope = "model_powertrain";
      copy.criteria = presetCriteriaForScope(copy.scope, context);
    }
    try { await signalPresets({ op: "save", preset: copy, expected_revision: null }); await refresh(); tab = "my"; }
    catch (e) { error = String(e); }
  }
  function editPreset(preset: SignalPreset) {
    editingPreset = structuredClone(preset);
    name = preset.name;
    family = preset.family;
    scope = preset.scope;
  }
  async function deletePreset(preset: SignalPreset) {
    try { await signalPresets({ op: "delete", id: preset.id, expected_revision: preset.revision }); await refresh(); }
    catch (e) { error = String(e); }
  }
</script>

{#if open}
<section class="preset-picker" aria-label="Signal presets">
  <header><div><p class="eyebrow">SIGNAL RECIPES</p><h2>Presets</h2><p>{context.make ?? "Any vehicle"} {context.model ?? ""}{context.year ? ` · ${context.year}` : ""}</p></div><button class="button secondary" onclick={onClose}>Close</button></header>
  <nav aria-label="Preset lists"><button class:active={tab === "suggested"} onclick={() => tab = "suggested"}>Suggested</button><button class:active={tab === "all"} onclick={() => tab = "all"}>All presets</button><button class:active={tab === "my"} onclick={() => tab = "my"}>My presets</button></nav>
  <div class="filters"><input aria-label="Search presets" placeholder="Search presets" bind:value={query} /><select aria-label="Filter by diagnostic task" bind:value={familyFilter}><option value="">All tasks</option><option value="engine_tuning">Engine tuning</option><option value="cvt_diagnostics">CVT diagnostics</option><option value="roughness_misfires">Roughness / misfires</option><option value="trims_air_fuel">Trims / air–fuel</option></select></div>
  <section class="vehicle-context" aria-label="Vehicle context"><h3>Vehicle context</h3><select aria-label="Saved profile context" bind:value={context.profile_id}><option value={null}>No saved profile</option>{#each profiles as profile (profile.id)}<option value={profile.id}>{profile.name}</option>{/each}</select><input placeholder="Make" aria-label="Make" bind:value={context.make} /><input placeholder="Model" aria-label="Model" bind:value={context.model} /><input placeholder="Year" aria-label="Year" type="number" bind:value={context.year} /><input placeholder="Engine" aria-label="Engine" bind:value={context.engine} /><input placeholder="Transmission" aria-label="Transmission" bind:value={context.transmission} /></section>
  {#if context.profile_id}<small>Matched saved vehicle: {profiles.find((profile) => profile.id === context.profile_id)?.name ?? context.profile_id}</small>{/if}
  <button class="button secondary update-suggestions" onclick={updateSuggestions}>Update suggestions for this vehicle</button>
  <small>Manual context filters suggestions; it does not verify an ECU or OEM request.</small>
  <div class="preset-list">{#each visible as preset (preset.id)}<article><div><strong>{preset.name}</strong><small>{preset.scope} · {preset.family} · {preset.channels.length} suggested channels</small><small>{preset.origin === "builtin" ? "Built-in recipe" : "Custom recipe"}</small></div><div class="actions"><button class="button secondary" onclick={() => void makePreview(preset.id)}>Preview</button>{#if preset.origin === "builtin"}<button class="button secondary" onclick={() => void copyPreset(preset)}>Copy</button>{:else}<button class="button secondary" onclick={() => editPreset(preset)}>Edit</button><button class="button secondary" onclick={() => void deletePreset(preset)}>Delete</button>{/if}</div></article>{/each}</div>
  {#if preview}<section class="preview" bind:this={previewElement}><h3>{preview.preset.name}{preview.local_only ? " · offline preview" : ` · ${preview.eligible.length} available / ${preview.skipped.length} skipped`}</h3>{#if preview.local_only}<p>Support is unknown until you connect. {preview.task_defining_total} task-defining channels will be checked; this staged intent does not claim vehicle support.</p>{:else}<p>Task-defining channels: {preview.task_defining_eligible}/{preview.task_defining_total}. Availability checked for this connection.</p>{/if}<label>Apply mode <select bind:value={mode}><option value="replace">Replace selection</option><option value="add">Add to selection</option></select></label>{#if preview.local_only}<details open><summary>To check on connection ({preview.skipped.length})</summary>{#each preview.skipped as channel}<p>{channel.concept}{channel.task_defining ? " · task-defining" : ""}</p>{/each}</details><button class="button secondary" onclick={clearStagedPreset}>Clear staged preset</button>{:else}<details><summary>Available ({preview.eligible.length})</summary>{#each preview.eligible as channel}<p>{channel.concept} · {channel.signal_id}</p>{/each}</details><details><summary>Skipped ({preview.skipped.length})</summary>{#each preview.skipped as channel}<p>{channel.concept} · {channel.reason ?? "Unavailable"}{channel.task_defining ? " · task-defining" : ""}</p>{/each}</details><button class="button primary" onclick={apply} disabled={!connected || !preview.can_apply}>Apply selection</button>{/if}</section>{/if}
  <form onsubmit={(event) => { event.preventDefault(); void save(); }}><h3>{editingPreset ? "Edit custom preset" : "Save current selection"}</h3>{#if editingPreset}<small>Changing the name, task, or scope keeps this preset’s saved channels.</small>{/if}<input aria-label="Preset name" placeholder="Preset name" bind:value={name} required /><select aria-label="Preset task" bind:value={family}><option value="engine_tuning">Engine tuning</option><option value="cvt_diagnostics">CVT diagnostics</option><option value="roughness_misfires">Roughness / misfires</option><option value="trims_air_fuel">Trims / air–fuel</option></select><select aria-label="Preset scope" bind:value={scope}><option value="generic">Generic</option><option value="model_powertrain">Model / powertrain</option><option value="exact_profile">Exact saved profile</option></select><button class="button secondary" disabled={!name.trim()}>Save preset</button></form>
  {#if error}<p role="alert">{error}</p>{/if}
</section>
{/if}

<style>
  .preset-picker{box-sizing:border-box;display:grid;gap:14px;width:100%;min-width:0;color:var(--text-primary)}.preset-picker>*{min-width:0}.preset-picker>header{position:sticky;top:0;z-index:2;padding:8px 12px;border:2px solid var(--accent);border-radius:8px;background:var(--surface-raised);box-shadow:0 6px 14px #0004}header,nav,form,.filters,.actions{display:flex;align-items:center;justify-content:space-between;gap:10px}.filters,.actions{justify-content:flex-start;flex-wrap:wrap}.filters input{flex:1 1 180px}.vehicle-context{display:grid;grid-template-columns:repeat(3,minmax(0,1fr));gap:8px;min-width:0}.vehicle-context h3{grid-column:1/-1}.vehicle-context input,.vehicle-context select{width:100%;box-sizing:border-box}.preset-list{display:grid;gap:8px;min-width:0}.preset-list article,.preview{box-sizing:border-box;min-width:0;padding:12px;border:1px solid var(--border);border-radius:8px}.preset-list article{display:grid;grid-template-columns:minmax(0,1fr) auto;align-items:start}.preset-list article>div:first-child{min-width:0;overflow-wrap:anywhere}h2,h3,p{margin:3px 0}small{display:block;color:var(--text-muted);margin-top:4px}nav{justify-content:flex-start;flex-wrap:wrap}nav button{background:transparent;color:inherit;border:0;padding:8px}nav .active{border-bottom:2px solid var(--accent)}input,select{min-width:0;padding:8px;background:var(--surface-raised);color:inherit;border:1px solid var(--border);border-radius:5px}.preview{overflow-wrap:anywhere;scroll-margin-top:110px}details{padding:6px}form{justify-content:flex-start;flex-wrap:wrap;border-top:1px solid var(--border);padding-top:12px}@media(max-width:640px){.vehicle-context{grid-template-columns:repeat(2,minmax(0,1fr))}.preset-list article{grid-template-columns:minmax(0,1fr)}.actions{justify-content:flex-start}}
</style>
