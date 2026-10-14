<script lang="ts">
  import { copyFullVin, vinDecodeLabels, type VehicleIdentitySnapshot } from "./vehicleProfiles";
  import {
    diagnosticOutcomeLabel, diagnosticReadLabel, formatHexByte, groupDiagnosticReadsByEcu,
    mode06MonitorSummary, type DiagnosticRead, type DiagnosticReport, type DiagnosticValue,
  } from "./vehicleDiagnostics";

  let { report, activeTab, identitySnapshot = null }: {
    report: DiagnosticReport;
    activeTab: "dtcs" | "identity" | "mode06" | "raw";
    identitySnapshot?: VehicleIdentitySnapshot | null;
  } = $props();

  let tabReads = $derived(report.reads.filter((read) => {
    if (activeTab === "dtcs") return read.kind.endsWith("dtcs");
    if (activeTab === "identity") return ["vin", "calibration_id", "ecu_name"].includes(read.kind);
    if (activeTab === "mode06") return read.kind.startsWith("mode06_");
    return true;
  }));
  let copyMessage = $state("");

  function valuesOf(read: DiagnosticRead, type: DiagnosticValue["type"]): DiagnosticValue[] {
    return read.values.filter((value) => value.type === type);
  }

  function payloadHex(bytes: number[]): string {
    return bytes.map(formatHexByte).join(" ");
  }

  function sourceLabel(source: VehicleIdentitySnapshot["identity_source"]): string {
    return {
      report_ecu: "VIN read from this report",
      same_generation_recognition: "Same-connection recognition",
      manual_profile: "Manual profile context",
      conflict: "Conflicting VINs · no single vehicle assigned",
      unassigned: "Unassigned vehicle",
    }[source];
  }

  async function copyVin(vin: string) {
    copyMessage = await copyFullVin(vin, (value) => navigator.clipboard.writeText(value))
      ? "Full VIN copied."
      : "Clipboard unavailable. The VIN remains visible above.";
  }
</script>

{#if identitySnapshot}
  <section class="report-identity" aria-label="Frozen vehicle context">
    <p class="eyebrow">CAPTURED VEHICLE CONTEXT</p>
    <p><strong>{sourceLabel(identitySnapshot.identity_source)}</strong> · {identitySnapshot.scope.replaceAll("_", " ")}{#if identitySnapshot.connection_generation !== null} · connection {identitySnapshot.connection_generation}{/if}{#if identitySnapshot.profile_id} · profile {identitySnapshot.profile_id} r{identitySnapshot.profile_revision ?? "?"}{/if}</p>
    {#if identitySnapshot.vin_conflict || identitySnapshot.identity_source === "conflict"}
      <p role="status">Conflicting VIN evidence was captured. No current profile has been substituted.</p>
    {:else if identitySnapshot.full_vin}
      <div class="full-vin"><span><small>Full VIN · {identitySnapshot.origin?.replaceAll("_", " ") ?? "stored source"}</small><code>{identitySnapshot.full_vin}</code></span><button class="button secondary" type="button" onclick={() => void copyVin(identitySnapshot.full_vin!)}>Copy VIN</button></div>
      {#if copyMessage}<small role="status">{copyMessage}</small>{/if}
    {:else if identitySnapshot.masked_vin}
      <p>Masked VIN {identitySnapshot.masked_vin} · full VIN is unavailable in this frozen snapshot.</p>
    {/if}
    {#if identitySnapshot.decoded_snapshot}
      {@const decoded = identitySnapshot.decoded_snapshot}
      {@const labels = vinDecodeLabels(decoded)}
      <p>Offline decode · {labels.manufacturer} · {labels.year} · {labels.checkDigit}</p>
      {#if decoded.wmi_assignment}<small>WMI source {decoded.wmi_assignment.source_id} · {decoded.wmi_assignment.source_revision} · decoder {decoded.decoder_revision} · data SHA-256 {decoded.wmi_dataset_sha256}</small>{/if}
    {/if}
    {#if identitySnapshot.owner_details.length}
      <details><summary>Owner-entered details</summary><ul>{#each identitySnapshot.owner_details as detail, index (`${detail.field}:${detail.entered_at_unix_ms}:${index}`)}<li>{detail.field.replaceAll("_", " ")}: {detail.value} · {detail.actor.replaceAll("_", " ")} · {new Date(detail.entered_at_unix_ms).toLocaleString()}{#if detail.note} · {detail.note}{/if}</li>{/each}</ul></details>
    {/if}
  </section>
{/if}

<div class="report-content">
  {#if !tabReads.length}
    <p class="diagnostic-empty">No results in this page.</p>
  {:else}
    {#each groupDiagnosticReadsByEcu(tabReads) as [ecu, reads]}
      <section class="diagnostic-ecu" aria-label={`Results from ECU ${ecu}`}>
        <header><h3>{ecu}</h3><span>{reads.length} read{reads.length === 1 ? "" : "s"}</span></header>
        {#each reads as read}
          <article class="diagnostic-read">
            <div class="diagnostic-read-heading">
              <h4>{diagnosticReadLabel(read)}</h4>
              <span class={`diagnostic-outcome ${read.outcome}`}>{diagnosticOutcomeLabel(read.outcome)}</span>
            </div>
            <p class="diagnostic-read-meta">{read.latency_ms} ms · Mode {read.service.toString(16).toUpperCase().padStart(2, "0")}{read.identifier === null ? "" : ` · ID ${formatHexByte(read.identifier)}`}</p>
            {#if read.negative_response_code !== null}<p class="diagnostic-detail">Negative response NRC {formatHexByte(read.negative_response_code)}</p>{/if}
            {#if read.detail}<p class="diagnostic-detail">{read.detail}</p>{/if}

            {#each valuesOf(read, "dtcList") as value}
              {#if value.type === "dtcList"}
                {#if read.outcome !== "positive"}
                  <p class="diagnostic-empty">Code count unknown · {diagnosticOutcomeLabel(read.outcome)}</p>
                {:else if value.payload.codes.length}
                  <div class="diagnostic-dtcs">{#each value.payload.codes as code}<code>{code}</code>{/each}</div>
                {:else}<p class="diagnostic-empty">No codes reported by this ECU.</p>{/if}
              {/if}
            {/each}
            {#each valuesOf(read, "identity") as value}
              {#if value.type === "identity"}<p class="diagnostic-identity"><span>{value.payload.label}</span><strong>{value.payload.value}</strong></p>{/if}
            {/each}
            {#each valuesOf(read, "mode06Bitmap") as value}
              {#if value.type === "mode06Bitmap"}<p class="diagnostic-bitmap">Page {formatHexByte(value.payload.start)} · {value.payload.test_ids.length} IDs {value.payload.has_more ? "· continuation advertised" : ""}</p>{/if}
            {/each}
            {#each valuesOf(read, "mode06Monitor") as value}
              {#if value.type === "mode06Monitor"}<p class="diagnostic-monitor">{mode06MonitorSummary(value.payload)}</p>{/if}
            {/each}

            {#if activeTab === "raw"}
              <details class="diagnostic-raw" open>
                <summary>Request and raw response</summary>
                <small>{read.route} · TX {payloadHex(read.request_payload)}</small>
                <code>{read.exact_adapter_command}</code>
                <small>RX {payloadHex(read.response_payload) || "(no payload)"}</small>
                <pre>{read.raw_response || "(no response bytes)"}</pre>
              </details>
            {/if}
          </article>
        {/each}
      </section>
    {/each}
  {/if}
</div>

<style>
  .report-content { min-width: 0; }
  .report-identity { display: grid; gap: 5px; margin: 0 0 12px; padding: 9px 10px; border: 1px solid var(--border); border-radius: 8px; background: var(--surface-deep); font-size: 10px; }
  .report-identity p, .report-identity small { margin: 0; color: var(--text-muted); overflow-wrap: anywhere; }
  .report-identity .eyebrow { color: var(--accent); font-size: 9px; font-weight: 700; letter-spacing: .12em; }
  .full-vin { display: flex; align-items: center; justify-content: space-between; gap: 8px; flex-wrap: wrap; }
  .full-vin span { display: grid; gap: 3px; min-width: 0; }
  .full-vin small { color: var(--text-muted); }
  .full-vin code { overflow-wrap: anywhere; font: 600 12px/1.4 ui-monospace, monospace; }
  .report-identity details ul { margin: 4px 0 0; padding-left: 18px; }
  .report-identity li { margin-top: 3px; }
  .diagnostic-ecu { margin: 0 0 12px; border: 1px solid var(--border); border-radius: 8px; overflow: hidden; }
  .diagnostic-ecu > header { display: flex; justify-content: space-between; gap: 10px; align-items: center; padding: 7px 10px; background: var(--surface-top); }
  .diagnostic-ecu > header h3 { margin: 0; font-size: 11px; }
  .diagnostic-ecu > header span { color: var(--text-muted); font-size: 9px; }
  .diagnostic-read { padding: 9px 10px; border-top: 1px solid var(--border-soft); }
  .diagnostic-read:first-of-type { border-top: 0; }
  .diagnostic-read-heading { display: flex; align-items: center; justify-content: space-between; gap: 10px; }
  .diagnostic-read-heading h4 { margin: 0; font-size: 11px; }
  .diagnostic-outcome { padding: 2px 6px; border-radius: 10px; color: var(--text-muted); background: var(--surface-raised); font-size: 9px; text-transform: capitalize; white-space: nowrap; }
  .diagnostic-outcome.positive { color: #b9e1d4; background: #1c3933; }
  .diagnostic-outcome.negative_response, .diagnostic-outcome.malformed_response, .diagnostic-outcome.transport_error { color: #ffd0cb; background: #492825; }
  .diagnostic-outcome.no_response, .diagnostic-outcome.temporarily_unavailable, .diagnostic-outcome.response_pending { color: #f5d8a1; background: #3b3327; }
  .diagnostic-read-meta, .diagnostic-detail, .diagnostic-bitmap, .diagnostic-monitor { margin: 4px 0 0; color: var(--text-muted); font-size: 9px; }
  .diagnostic-detail { color: #ffd0cb; }
  .diagnostic-dtcs { display: flex; flex-wrap: wrap; gap: 5px; margin-top: 7px; }
  .diagnostic-dtcs code { padding: 3px 6px; border: 1px solid var(--border); border-radius: 5px; color: var(--text); background: var(--surface-raised); font-size: 10px; }
  .diagnostic-identity { display: flex; flex-wrap: wrap; gap: 4px 12px; margin: 7px 0 0; font-size: 10px; }
  .diagnostic-identity span { min-width: 90px; color: var(--text-muted); }
  .diagnostic-identity strong { overflow-wrap: anywhere; font-weight: 600; }
  .diagnostic-empty, .diagnostic-bitmap { margin: 7px 0; color: var(--text-muted); font-size: 10px; }
  .diagnostic-raw { margin-top: 8px; color: var(--text-muted); font-size: 9px; }
  .diagnostic-raw summary { cursor: pointer; }
  .diagnostic-raw small, .diagnostic-raw code, .diagnostic-raw pre { display: block; margin: 5px 0 0; overflow-wrap: anywhere; }
  .diagnostic-raw code, .diagnostic-raw pre { color: var(--text-secondary); font: 9px/1.4 ui-monospace, monospace; white-space: pre-wrap; }
  .diagnostic-raw pre { max-height: 150px; overflow: auto; }
  .button.secondary { flex: 0 0 auto; color: inherit; background: transparent; border: 1px solid var(--border); border-radius: 6px; padding: 5px 8px; font: inherit; cursor: pointer; }
</style>
