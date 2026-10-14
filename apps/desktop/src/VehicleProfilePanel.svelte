<script lang="ts">
  import {
    assignManualProfileVin,
    canApplyProfileResume,
    copyFullVin,
    decodeVehicleVinOffline,
    getVehicleIdentity,
    linkLegacyVehicleVin,
    setManualVehicleDetail,
    vehicleIdentityVinPresentation,
    vinDecodeLabels,
    type ManualVehicleField,
    type OemVehicleProfile,
    type ProfileResumePreview,
    type VehicleIdentityRecord,
    type VehicleIdentityObservation,
    type VehicleProfile,
    type VehicleProfileStatus,
  } from "./vehicleProfiles";

  let {
    profiles = [],
    catalogProfiles = [],
    status = null,
    preview = null,
    busy = false,
    error = null,
    onRecognize = () => {},
    onSave = (_name: string, _manualAssociation: boolean, _catalogVehicleId: string | null) => {},
    onPrepareResume = (_profileId: string) => {},
    onApplyResume = (_previewId: string, _revision: number, _generation: number) => {},
    onDiscardResume = (_previewId: string) => {},
    onManualAssociate = (_profileId: string) => {},
    onContinueSelection = () => {},
    onIdentityChanged = () => {},
  }: {
    profiles?: VehicleProfile[];
    catalogProfiles?: OemVehicleProfile[];
    status?: VehicleProfileStatus | null;
    preview?: ProfileResumePreview | null;
    busy?: boolean;
    error?: string | null;
    onRecognize?: () => void;
    onSave?: (name: string, manualAssociation: boolean, catalogVehicleId: string | null) => void;
    onPrepareResume?: (profileId: string) => void;
    onApplyResume?: (previewId: string, revision: number, generation: number) => void;
    onDiscardResume?: (previewId: string) => void;
    onManualAssociate?: (profileId: string) => void;
    onContinueSelection?: () => void;
    onIdentityChanged?: () => void | Promise<void>;
  } = $props();

  let profileName = $state("");
  let manualAssociation = $state(false);
  let catalogVehicleId = $state("");
  let selectedProfileId = $state("");
  let identityProfileId = $state<string | null>(null);
  let identityRecord = $state<VehicleIdentityRecord | null>(null);
  let identityLoading = $state(false);
  let identityBusy = $state(false);
  let identityError = $state("");
  let identityMessage = $state("");
  let identityVinInput = $state("");
  let manualField = $state<ManualVehicleField>("make");
  let manualValue = $state("");
  let manualNote = $state("");
  let copyMessage = $state("");
  let recognized = $derived(status?.current_observation ?? null);
  let generation = $derived(status?.obd_connection_generation ?? -1);
  let canApply = $derived(canApplyProfileResume(preview, status?.obd_connected ?? false, status?.profile_standby ?? false, generation));

  function save() {
    const name = profileName.trim();
    if (!name || busy) return;
    onSave(name, manualAssociation, catalogVehicleId || null);
    profileName = "";
    manualAssociation = false;
  }

  function profileLabel(profile: VehicleProfile): string {
    return [profile.masked_vin, profile.scope === "simulator" ? "Simulator" : profile.scope === "can_only" ? "CAN-only" : null]
      .filter(Boolean).join(" · ") || "Identity unavailable";
  }

  function identityLabel(observation: VehicleIdentityObservation): string {
    if (observation.conflicting_vin_observations) return "ECUs reported conflicting VINs";
    return observation.masked_vin ?? (observation.identity_supported ? "VIN unavailable" : "Identity reads unsupported");
  }
  async function toggleIdentityDetails(profile: VehicleProfile) {
    if (identityProfileId === profile.id) {
      identityProfileId = null;
      identityRecord = null;
      return;
    }
    identityProfileId = profile.id;
    identityRecord = null;
    identityError = "";
    identityMessage = "";
    copyMessage = "";
    identityVinInput = "";
    if (!profile.vin_digest) return;
    identityLoading = true;
    try {
      const record = await getVehicleIdentity(profile.scope, profile.vin_digest);
      if (identityProfileId === profile.id) identityRecord = record;
    } catch (cause) {
      if (identityProfileId === profile.id) identityError = String(cause);
    } finally {
      identityLoading = false;
    }
  }

  async function saveAssignedVin(profile: VehicleProfile) {
    if (identityBusy || !identityVinInput.trim()) return;
    identityBusy = true;
    identityError = "";
    identityMessage = "";
    try {
      identityRecord = profile.vin_digest
        ? await linkLegacyVehicleVin(profile.id, profile.revision, identityVinInput.trim())
        : await assignManualProfileVin(profile.id, profile.revision, identityVinInput.trim());
      identityVinInput = "";
      identityMessage = profile.vin_digest
        ? "VIN linked to the existing digest; it remains owner-entered evidence."
        : "VIN assigned to this manual profile; it is not ECU verification.";
      await onIdentityChanged();
    } catch (cause) {
      identityError = String(cause);
    } finally {
      identityBusy = false;
    }
  }

  async function decodeStoredVin(profile: VehicleProfile) {
    if (identityBusy || !profile.vin_digest) return;
    identityBusy = true;
    identityError = "";
    identityMessage = "";
    try {
      identityRecord = await decodeVehicleVinOffline(profile.scope, profile.vin_digest);
      identityMessage = "Decoded locally from the stored VIN and bundled WMI data.";
    } catch (cause) {
      identityError = String(cause);
    } finally {
      identityBusy = false;
    }
  }

  async function saveManualDetail(profile: VehicleProfile) {
    if (identityBusy || !profile.vin_digest || !manualValue.trim()) return;
    identityBusy = true;
    identityError = "";
    identityMessage = "";
    try {
      identityRecord = await setManualVehicleDetail(profile.scope, profile.vin_digest, manualField, manualValue.trim(), manualNote.trim() || null);
      manualValue = "";
      manualNote = "";
      identityMessage = "Owner-entered detail saved separately from decoded and ECU data.";
      await onIdentityChanged();
    } catch (cause) {
      identityError = String(cause);
    } finally {
      identityBusy = false;
    }
  }

  async function copyIdentityVin(vin: string) {
    const copied = await copyFullVin(vin, (value) => navigator.clipboard.writeText(value));
    copyMessage = copied ? "Full VIN copied." : "Clipboard unavailable. The full VIN remains visible above.";
  }
</script>

<section class="vehicle-profiles" aria-labelledby="vehicle-profiles-title">
  <header>
    <div>
      <p class="eyebrow">LOCAL VEHICLE SETTINGS</p>
      <h2 id="vehicle-profiles-title">Vehicle and ECU profiles</h2>
      <p class="intro">Save signal choices for this vehicle. Recognition only prepares a preview; Resume applies supported logging signals after review.</p>
    </div>
  </header>

  {#if error}<p class="error" role="alert">{error}</p>{/if}

  {#if status?.obd_connected}
    <div class="connection" role="status">
      <span class:standby={status.profile_standby} class="connection-dot"></span>
      <span>{status.profile_standby ? "Connected in profile standby" : "OBD connected"}</span>
      <span class="generation">Connection {generation}</span>
    </div>
    {#if recognized}
      <div class="identity" role="status">
        <span class="eyebrow">CURRENT RECOGNITION</span>
        <strong>{identityLabel(recognized)}</strong>
        <span>{recognized.ecu_variants.length} responding ECU{recognized.ecu_variants.length === 1 ? "" : "s"}; identity status: {recognized.vin_status.replaceAll("_", " ")}</span>
      </div>
    {/if}
    <div class="actions">
      <button class="button secondary" type="button" onclick={onRecognize} disabled={busy}>
        {busy ? "Recognizing…" : "Recognize vehicle"}
      </button>
      {#if status.profile_standby}
        <button class="button secondary" type="button" onclick={onContinueSelection} disabled={busy}>Continue with signal selection</button>
      {/if}
    </div>
    {#if status.profile_standby && !recognized}
      <p class="notice">Polling is paused while you identify the vehicle. If identity reads are unavailable, continue with generic signal selection or manually associate a profile.</p>
    {:else if !status.profile_standby && profiles.length > 0}
      <p class="notice">This connection is already polling. To apply a saved profile, reconnect in profile standby so the preview can be applied without an old preset polling first.</p>
    {/if}

    <form class="save-form" onsubmit={(event) => { event.preventDefault(); save(); }}>
      <label for="vehicle-profile-name">Save current selection</label>
      <div class="save-row">
        <input id="vehicle-profile-name" bind:value={profileName} maxlength="120" placeholder="e.g. 2011 Ford Flex" disabled={busy} />
        <button class="button primary" type="submit" disabled={busy || !profileName.trim()}>Save profile</button>
      </div>
      {#if !recognized || !recognized.vin_digest}
        <label class="manual"><input type="checkbox" bind:checked={manualAssociation} disabled={busy} /> I am manually associating this profile with the connected vehicle</label>
      {/if}
      <details class="catalog-association">
        <summary>Research catalog association (optional)</summary>
        <label for="vehicle-profile-catalog">Catalog vehicle</label>
        <select id="vehicle-profile-catalog" bind:value={catalogVehicleId} disabled={busy}>
          <option value="">No catalog association</option>
          {#each catalogProfiles as catalogProfile (catalogProfile.vehicle_id)}
            <option value={catalogProfile.vehicle_id}>{catalogProfile.brand} {catalogProfile.model}{catalogProfile.model_year ? ` (${catalogProfile.model_year})` : ""} · {catalogProfile.vehicle_id}</option>
          {/each}
        </select>
        <p class="notice">This selects a catalog scope for research checks; it does not prove vehicle or ECU identity.</p>
      </details>
    </form>
  {:else}
    <p class="notice">Connect an OBD adapter to recognize a vehicle and save a profile.</p>
  {/if}

  {#if profiles.length}
    <ul class="profile-list" aria-label="Saved vehicle profiles">
      {#each profiles as profile (profile.id)}
        {@const summary = status?.profile_matches.find((match) => match.profile_id === profile.id)}
        <li class:selected={selectedProfileId === profile.id}>
          <div class="profile-description">
            <strong>{profile.name}</strong>
            <span>{profileLabel(profile)} · revision {profile.revision}</span>
            {#if summary}<span class="match" class:exact={summary.profile_match.state === "exact"}>{summary.profile_match.state.replaceAll("_", " ")}</span>{/if}
          </div>
          <div class="profile-actions">
            <button class="button secondary" type="button" onclick={() => { selectedProfileId = profile.id; onPrepareResume(profile.id); }} disabled={busy || !status?.obd_connected}>Preview</button>
            <button class="button secondary" type="button" onclick={() => void toggleIdentityDetails(profile)} disabled={busy}>{identityProfileId === profile.id ? "Hide VIN details" : "VIN details"}</button>
            {#if summary?.profile_match.state === "identity_unavailable"}
              <button class="text-button" type="button" onclick={() => onManualAssociate(profile.id)} disabled={busy}>Associate manually</button>
            {/if}
          </div>
          {#if identityProfileId === profile.id}
            <section class="identity-details" aria-label={`VIN details for ${profile.name}`}>
              <h4>Vehicle identity · {profile.scope.replaceAll("_", " ")}</h4>
              {#if identityLoading}<p class="notice">Loading stored identity…</p>{/if}
              {#if identityError}<p class="error" role="alert">{identityError}</p>{/if}
              {#if identityRecord}
                {@const vin = vehicleIdentityVinPresentation({ ...identityRecord, masked_vin: profile.masked_vin })}
                <div class="full-vin-row">
                  <span><small>{vin.label}</small><code>{vin.value}</code></span>
                  {#if vin.full}<button class="button secondary" type="button" onclick={() => void copyIdentityVin(vin.value)}>Copy VIN</button>{/if}
                </div>
                {#if copyMessage}<p class="notice" role="status">{copyMessage}</p>{/if}
                {#if identityRecord.full_vin && identityRecord.decoder_snapshot}
                  {@const decode = identityRecord.decoder_snapshot}
                  {@const labels = vinDecodeLabels(decode)}
                  <dl class="decode-grid">
                    <dt>Manufacturer</dt><dd>{labels.manufacturer}</dd>
                    {#if decode.wmi_assignment}<dt>WMI</dt><dd>{decode.wmi_assignment.wmi_key} · {decode.wmi_assignment.source_id} · {decode.wmi_assignment.source_revision}</dd>{/if}
                    {#if decode.wmi_assignment?.manufacturer_assignment_country}<dt>Manufacturer assignment country</dt><dd>{decode.wmi_assignment.manufacturer_assignment_country}</dd>{/if}
                    <dt>Model year</dt><dd>{labels.year}</dd>
                    <dt>Check digit</dt><dd>{labels.checkDigit}</dd>
                    <dt>Decoder</dt><dd>{decode.decoder_revision}</dd>
                    <dt>WMI data SHA-256</dt><dd><code>{decode.wmi_dataset_sha256}</code></dd>
                  </dl>
                {:else if identityRecord.full_vin}
                  <button class="button secondary" type="button" onclick={() => void decodeStoredVin(profile)} disabled={identityBusy}>{identityBusy ? "Decoding…" : "Decode stored VIN offline"}</button>
                  <p class="notice">This uses the bundled WMI file and local VIN rules. Unknown WMI, model, engine and trim values remain unknown.</p>
                {/if}
                {#if identityRecord.origin_evidence.length}
                  <details><summary>VIN evidence</summary><ul class="identity-evidence-list">
                    {#each identityRecord.origin_evidence as evidence}
                      <li>{evidence.origin.replaceAll("_", " ")} · {new Date(evidence.recorded_at_unix_ms).toLocaleString()}{evidence.ecu_id ? ` · ECU ${evidence.ecu_id}` : ""}{evidence.report_id ? ` · report ${evidence.report_id}` : ""}{evidence.actor ? ` · ${evidence.actor.replaceAll("_", " ")}` : ""}</li>
                    {/each}
                  </ul></details>
                {/if}
                <section class="owner-details" aria-label="Owner-entered vehicle details">
                  <h5>Owner-entered details</h5>
                  {#if identityRecord.owner_details.length}
                    <ul class="owner-detail-list">
                      {#each identityRecord.owner_details as detail, index (`${detail.field}:${detail.entered_at_unix_ms}:${index}`)}
                        <li><strong>{detail.field.replaceAll("_", " ")}: {detail.value}</strong><span>{detail.actor.replaceAll("_", " ")} · {new Date(detail.entered_at_unix_ms).toLocaleString()}</span>{#if detail.note}<small>{detail.note}</small>{/if}</li>
                      {/each}
                    </ul>
                  {:else}<p class="notice">No owner-entered details.</p>{/if}
                  <form class="manual-detail-form" onsubmit={(event) => { event.preventDefault(); void saveManualDetail(profile); }}>
                    <label>Field<select bind:value={manualField} disabled={identityBusy}><option value="make">Make</option><option value="model">Model</option><option value="model_year">Model year</option><option value="engine">Engine</option><option value="trim">Trim</option></select></label>
                    <label>Value<input bind:value={manualValue} maxlength="120" disabled={identityBusy} /></label>
                    <label>Note (optional)<input bind:value={manualNote} maxlength="300" disabled={identityBusy} /></label>
                    <button class="button secondary" type="submit" disabled={identityBusy || !manualValue.trim()}>Save manual detail</button>
                  </form>
                </section>
              {:else if !identityLoading && profile.vin_digest}
                <p class="notice">Only the profile VIN digest is available; its original VIN cannot be reversed.</p>
              {:else if !profile.vin_digest && profile.manual_association}
                <p class="notice">This profile has no VIN hash. Assigning a VIN stores an owner-entered identity and does not prove ECU support.</p>
              {:else if !profile.vin_digest}
                <p class="notice">A VIN can only be manually assigned to a profile that was explicitly associated by the owner.</p>
              {/if}
              {#if profile.vin_digest && !identityRecord?.full_vin}
                <form class="assign-vin-form" onsubmit={(event) => { event.preventDefault(); void saveAssignedVin(profile); }}>
                  <label for={`profile-vin-${profile.id}`}>Link full VIN to this saved hash</label>
                  <input id={`profile-vin-${profile.id}`} bind:value={identityVinInput} autocomplete="off" maxlength="17" placeholder="17-character VIN" disabled={identityBusy || identityLoading} />
                  <p class="notice">The service accepts this only if the VIN’s canonical digest exactly matches the saved profile.</p>
                  <button class="button secondary" type="submit" disabled={identityBusy || identityLoading || !identityVinInput.trim()}>Link legacy VIN</button>
                </form>
              {:else if !profile.vin_digest && profile.manual_association}
                <form class="assign-vin-form" onsubmit={(event) => { event.preventDefault(); void saveAssignedVin(profile); }}>
                  <label for={`profile-vin-${profile.id}`}>Assign full VIN manually</label>
                  <input id={`profile-vin-${profile.id}`} bind:value={identityVinInput} autocomplete="off" maxlength="17" placeholder="17-character VIN" disabled={identityBusy} />
                  <button class="button secondary" type="submit" disabled={identityBusy || !identityVinInput.trim()}>Assign VIN</button>
                </form>
              {/if}
              {#if identityMessage}<p class="notice" role="status">{identityMessage}</p>{/if}
            </section>
          {/if}
        </li>
      {/each}
    </ul>
  {/if}

  {#if preview}
    <section class="preview" aria-labelledby="profile-preview-title">
      <header>
        <div>
          <p class="eyebrow">RESUME PREVIEW · {preview.profile_match.state.replaceAll("_", " ")}</p>
          <h3 id="profile-preview-title">{profiles.find((profile) => profile.id === preview.profile_id)?.name ?? "Saved selection"}</h3>
          <p>Profile revision {preview.profile_revision} · connection {preview.obd_connection_generation}</p>
        </div>
        <button class="text-button" type="button" onclick={() => onDiscardResume(preview.preview_id)} disabled={busy}>Cancel preview</button>
      </header>
      <div class="preview-counts">
        <div><strong>{preview.eligible_log_refs.length}</strong><span>Logging signals</span></div>
        <div><strong>{preview.eligible_graph_refs.length}</strong><span>Graph signals</span></div>
        <div><strong>{preview.skipped.length}</strong><span>Skipped for review</span></div>
      </div>
      {#if preview.eligible_log_refs.length}
        <details open>
          <summary>Eligible logging signals</summary>
          <ul>{#each preview.eligible_log_refs as id}<li><code>{id}</code></li>{/each}</ul>
        </details>
      {/if}
      {#if preview.eligible_graph_refs.length}
        <details>
          <summary>Eligible graph signals</summary>
          <ul>{#each preview.eligible_graph_refs as id}<li><code>{id}</code></li>{/each}</ul>
        </details>
      {/if}
      {#if preview.skipped.length}
        <details open>
          <summary>Skipped selections</summary>
          <ul class="skipped-list">
            {#each preview.skipped as item}
              <li><code>{item.reference}</code><span>{item.detail}</span></li>
            {/each}
          </ul>
        </details>
      {/if}
      <div class="actions">
        <button class="button primary" type="button" onclick={() => onApplyResume(preview.preview_id, preview.profile_revision, preview.obd_connection_generation)} disabled={busy || !canApply}>Resume selection</button>
        {#if !status?.profile_standby}<span class="notice inline">Reconnect in profile standby to apply.</span>{/if}
      </div>
    </section>
  {/if}
</section>

<style>
  .vehicle-profiles { display: grid; gap: 1rem; color: var(--text-primary, #e9eef5); }
  header, .actions, .save-row, .connection, .profile-list li, .profile-actions, .preview-counts, .skipped-list li { display: flex; align-items: center; justify-content: space-between; gap: .75rem; }
  header { align-items: flex-start; }
  h2, h3, p { margin: 0; }
  h2 { font-size: 1.15rem; }
  h3 { font-size: 1rem; }
  .intro, .notice, .profile-description span, .identity span:last-child, .preview header p, .skipped-list span { color: var(--text-muted, #9eabb9); font-size: .85rem; }
  .eyebrow { color: #83a4c8; font-size: .7rem; font-weight: 700; letter-spacing: .1em; }
  .connection, .identity, .notice, .preview { border: 1px solid var(--border, #34404d); border-radius: .55rem; padding: .75rem; }
  .connection { justify-content: flex-start; }
  .connection-dot { width: .55rem; height: .55rem; border-radius: 50%; background: #64be8b; }
  .connection-dot.standby { background: #e2b55e; }
  .generation { margin-left: auto; color: var(--text-muted, #9eabb9); font-size: .75rem; }
  .identity { display: grid; gap: .25rem; }
  .identity strong { font-size: 1.05rem; }
  .notice { line-height: 1.45; }
  .save-form { display: grid; gap: .5rem; }
  .save-form > label:first-child { font-weight: 600; }
  .save-row input { flex: 1; min-width: 0; border: 1px solid var(--border, #34404d); background: var(--surface, #151c24); color: inherit; border-radius: .4rem; padding: .55rem .65rem; }
  .manual { display: flex; align-items: center; gap: .45rem; color: var(--text-muted, #9eabb9); font-size: .8rem; }
  .catalog-association { display: grid; gap: .4rem; }
  .catalog-association select { width: 100%; border: 1px solid var(--border, #34404d); background: var(--surface, #151c24); color: inherit; border-radius: .4rem; padding: .55rem .65rem; }
  .button, .text-button { border-radius: .4rem; cursor: pointer; font: inherit; }
  .button { border: 1px solid transparent; padding: .5rem .75rem; font-weight: 600; }
  .button.primary { color: #101820; background: #9cc7ee; }
  .button.secondary { color: inherit; background: transparent; border-color: var(--border, #34404d); }
  .button:disabled, .text-button:disabled { opacity: .45; cursor: default; }
  .text-button { border: 0; padding: .3rem; color: #9cc7ee; background: transparent; }
  .profile-list { display: grid; gap: .45rem; margin: 0; padding: 0; list-style: none; }
  .profile-list li { align-items: flex-start; border: 1px solid var(--border, #34404d); border-radius: .5rem; padding: .65rem; }
  .profile-list li { flex-wrap: wrap; }
  .profile-list li.selected { border-color: #789fc2; }
  .profile-description { display: grid; gap: .2rem; }
  .profile-description strong { font-size: .9rem; }
  .match { text-transform: capitalize; }
  .match.exact { color: #78d4a0; }
  .profile-actions { justify-content: flex-end; flex-wrap: wrap; }
  .identity-details { flex: 1 0 100%; display: grid; gap: .55rem; padding: .65rem; border-top: 1px solid var(--border, #34404d); }
  .identity-details h4, .owner-details h5 { margin: 0; font-size: .86rem; }
  .full-vin-row { display: flex; align-items: center; justify-content: space-between; gap: .6rem; flex-wrap: wrap; }
  .full-vin-row span { display: grid; gap: .2rem; min-width: 0; }
  .full-vin-row small, .decode-grid dt, .owner-detail-list span, .owner-detail-list small { color: var(--text-muted, #9eabb9); font-size: .75rem; }
  .full-vin-row code, .decode-grid code { overflow-wrap: anywhere; color: var(--text-primary, #e9eef5); font: 600 .9rem/1.4 ui-monospace, monospace; }
  .decode-grid { display: grid; grid-template-columns: minmax(8rem, 1fr) minmax(0, 2fr); gap: .28rem .6rem; margin: 0; font-size: .78rem; }
  .decode-grid dd { margin: 0; overflow-wrap: anywhere; }
  .owner-details { display: grid; gap: .4rem; padding-top: .4rem; border-top: 1px solid var(--border, #34404d); }
  .owner-detail-list, .identity-evidence-list { display: grid; gap: .35rem; margin: 0; padding-left: 1.1rem; }
  .owner-detail-list li { display: grid; gap: .14rem; }
  .owner-detail-list li strong { font-size: .8rem; }
  .manual-detail-form, .assign-vin-form { display: grid; gap: .4rem; }
  .manual-detail-form label, .assign-vin-form label { display: grid; gap: .2rem; color: var(--text-muted, #9eabb9); font-size: .76rem; }
  .manual-detail-form input, .manual-detail-form select, .assign-vin-form input { min-width: 0; border: 1px solid var(--border, #34404d); background: var(--surface, #151c24); color: inherit; border-radius: .4rem; padding: .45rem .55rem; }
  .identity-evidence-list li { color: var(--text-muted, #9eabb9); font-size: .75rem; overflow-wrap: anywhere; }
  .preview { display: grid; gap: .7rem; }
  .preview-counts { justify-content: flex-start; flex-wrap: wrap; }
  .preview-counts div { min-width: 6rem; display: grid; gap: .15rem; border-radius: .4rem; background: var(--surface, #151c24); padding: .5rem; }
  .preview-counts strong { font-size: 1.05rem; }
  .preview-counts span, summary { color: var(--text-muted, #9eabb9); font-size: .8rem; }
  details ul { margin: .4rem 0 0; padding-left: 1.3rem; }
  .skipped-list { display: grid; gap: .4rem; }
  .skipped-list li { justify-content: flex-start; align-items: baseline; }
  .notice.inline { border: 0; padding: 0; }
  .error { color: #ff9a9a; }
  @media (max-width: 760px) { .profile-list li { flex-direction: column; align-items: stretch; } .profile-actions { justify-content: flex-start; } .identity-details { flex-basis: auto; } .decode-grid { grid-template-columns: 1fr; } }
</style>
