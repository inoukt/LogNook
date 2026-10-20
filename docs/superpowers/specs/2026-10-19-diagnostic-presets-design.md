# Diagnostic signal presets — draft design

Status: approved for implementation by the owner on 2026-10-19 with Luna high agents.

## Purpose

Add a compact **Presets** button to the top bar. A technician chooses a diagnostic
task and LogNook suggests useful channel sets for the vehicle, powertrain, ECU,
and connected adapter. Users can create generic or vehicle-specific presets from
their current selections. Keep the existing Profiles identity/history workflow.

## User experience

- Put Presets beside Profiles. Open a workspace-sized dialog, rather than putting
  another list of long cards in the narrow signal sidebar.
- At the top show **Vehicle: [current vehicle or Choose vehicle]** with Change.
  Reuse recognized/profile identity and owner-entered details when present.
- If context is missing, offer make, model, year, engine, and transmission.
  Generic presets remain browsable without filling out a vehicle form. Ask only
  for fields needed to distinguish the requested model-specific suggestions.
- Use tabs **Suggested**, **All presets**, and **My presets**, plus a small task
  filter. Compact rows show name, scope, channel availability, and Preview.
- Preview displays eligible channels, missing channels and reasons, and the
  proposed logging/graph selections. **Replace selection** is the default;
  **Add to selection** is available and deduplicates existing channels.
- Apply changes the signal selection. Recording remains a separate action.
- Offline browsing and editing work. A selected offline recipe is staged for
  resolution after the next connection discovers real support; it does not
  fabricate a supported-signal list.
- Preset editing has **Save current selection**, name, task, and scope:
  **Generic**, **Model/powertrain**, or **This vehicle**. Built-ins can be copied
  into My presets. Editing a copy never modifies its built-in source.
- Detailed decoder/provenance information stays expandable. Use availability
  labels such as `8/12 available`, `Partial`, and `Needs vehicle details`.

## Starter library

These are channel concepts, not universal OEM identifiers or promised support.
Each recipe has a compact Core set and optional Extended channels.

| Family | Core concepts | Optional enhanced concepts |
| --- | --- | --- |
| Engine tuning | RPM, load, throttle, MAP/baro, MAF, lambda, ignition timing, coolant/IAT | Boost target/actual, knock, fuel rail pressure, injector data, wastegate, oil data |
| CVT diagnostics | Available transmission temperature, input/output speeds and ratio channels; RPM/load as context | Ratio target/actual, lockup/slip, pressure target/actual, CVT-specific states |
| Roughness / misfires | Available cylinder roughness/misfire channels; RPM/load, timing and trims as context | Learned/corrected timing and related model-specific channels; separate Mode 06 snapshot where supported and decoded |
| Trims / air–fuel | Available STFT/LTFT per bank, lambda/O2, RPM/load, MAF/MAP and coolant | Fuel-pressure target/actual, injector data and bank/cylinder detail |

## Shared engine context: knock and companion channels

All four initial families include an optional **Knock / timing correction**
channel by default when an applicable eligible definition is available. In CVT
presets this is engine context rather than a required transmission channel.
Missing optional knock data does not block an otherwise useful preset.

Keep instantaneous knock retard/correction, learned knock correction, knock
counts, and advance multipliers as separate concepts. For Subaru, feedback knock
correction, fine knock learning and DAM describe different mechanisms; DAM is
not a degree-valued knock-retard signal. Keep source names, signs, units and
provenance visible. Never substitute generic ignition advance or raw knock sensor
voltage for knock retard, and never apply a universal sign/threshold rule.
References: [COBB knock monitoring](https://cobbtuning.atlassian.net/wiki/spaces/PRS/pages/338723039/Subaru%2BKnock%2BMonitoring),
[COBB logging overview](https://www.cobbtuning.com/subaru-data-logging-overview/).

The shared engine-context bundle also prefers RPM, load, final ignition timing,
driver demand/throttle, IAT/charge-air temperature and coolant, with voltage as a
slow context channel. Deduplicate slots when the selected family already contains
them. Model-specific learned corrections and cylinder-level knock detail belong
in Extended unless the recipe makes them a diagnostic priority.

Companion priorities for the initial library:

- **Target versus actual:** lambda, boost/MAP, fuel pressures, cam positions and
  CVT ratio/pressure wherever both quantities have applicable definitions. Keep
  commanded, measured and derived values labeled separately.
- **Fuel control state:** bank-specific trims, closed/open-loop state and
  available injection/pressure channels alongside the air–fuel measurements.
- **Operating context:** pedal/throttle, load, RPM, speed and selected/actual
  transmission state so the same correction can be placed in context.
- **Thermal/electrical context:** intake/charge-air, coolant and transmission/oil
  temperatures, plus system voltage; poll slow-changing context at lower priority.
- **Cylinder/bank detail:** preserve separate cylinder counters/corrections and
  bank fuel data; do not merge them into one misleading aggregate.

Core sets stay small. Scheduler priorities favor the task's rapidly changing
channels; slow context remains logged at a suitable lower cadence. Display
measured rates, stale-channel and timeout state instead of assuming that adding
more signals has no rate cost. Reuse existing markers/triggers for optional
events based on an explicitly defined signal condition; no global knock alarm
threshold is built into these recipes.

Future display additions can include target/actual graph pairs and an RPM/load
scatter view colored by knock correction or lambda. These are optional display
defaults, not a prerequisite for the first preset picker.

Only actual applicable channels appear as available. A CVT preset with no CVT
channels must say so; generic RPM alone does not make it a working CVT diagnostic
preset. Roughness counters and slow Mode 06 monitor results are distinct sources.
Do not invent misfire counts from RPM, add universal AFR conversions, or assume
transmission type from make/model alone.

Additional families such as cooling, starting/charging, idle and boost/knock can
follow once the initial four have useful mappings and measured acquisition rates.

## Vehicle matching and definition selection

1. Exact saved vehicle/ECU evidence and a compatible preset revision.
2. Explicit model/year/engine/transmission applicability.
3. Explicit generation/powertrain applicability where source evidence supports it.
4. Generic supported OBD fallback.

Manual vehicle choices are applicability hints, not ECU validation. Existing VIN
decoding does not reliably provide model, engine, or transmission. Add Transmission
to owner-entered vehicle details and preserve its owner/Agent provenance.
Related-platform mappings remain research leads. Each recipe stores concept
labels and exact source definition references/alternatives, not copied decoders.
Do not introduce a second PID database or a new general concept-mapping subsystem
just to ship the first four recipes.

Generic channels require current ECU-reported support. OEM automatic preset
channels require the project's existing source/applicability/verification rules.
Imported unverified candidates may be shown as optional research leads with their
status, but never enter an automatic preset by being suggested. Existing explicit
manual Subaru research logging remains available through its current workflow.
Show adapter/protocol mismatches clearly, including CAN versus SSM K-Line.

## Persistence and execution

- Keep custom presets and staged preset intent in the existing profiles.sqlite
  application store. Ship versioned first-party recipes as a small data file.
  The authoritative PID SQLite database continues to own signal definitions.
- A preset has ID, revision, name, task, scope, vehicle criteria, concept slots,
  source references and separate logging/graph selections. Custom snapshots reuse
  ProfileSelectionSnapshot rather than duplicating its representation.
- Preview binds the preset revision, context revision, definition revisions, and
  OBD connection generation. Apply rechecks eligibility and rejects stale work.
- Existing profile selection replacement is standby-only. To support changing a
  preset on an idle live connection, add an atomic selection-switch command to
  the existing acquisition owner. Validate first, acknowledge the new schedule,
  then publish the corresponding selection state. A failed switch retains the
  previous schedule. Never open a second adapter session for presets.
- Block Apply during recording/playback or conflicting adapter operations, while
  keeping browsing and saving available. Starting a CAN capture, raw transmit,
  diagnostic reset or DTC clear is outside preset application.
- Core and Extended are selection sizes, not fixed sampling-rate promises. Keep
  scheduling independent and show measured rates when data is available.
- Preserve exact raw data and decoder provenance in recordings. Preset changes
  never alter existing recordings or source catalog rows.

## Agent Access / MCP

Expose one typed signal_presets tool with list, get, suggest, preview, apply,
save, delete, status and cancel operations. UI and MCP call the same resolver,
store, preview validation and acquisition owner. Use existing authorization,
operation ownership, lock/expiry cancellation and evidence conventions. Browsing
and suggesting never transmit. Saving a preset never promotes OEM validation.

## Initial acceptance

- Generic presets are usable without vehicle entry; model suggestions ask for
  missing context and do not silently choose a different vehicle or powertrain.
- Named built-in and custom presets are easy to find, preview, copy and save.
- Missing task-defining channels are disclosed, including all-CVT-channel absence.
- Add deduplicates; Replace applies the previewed set. Failed or stale Apply
  leaves the previous selection intact.
- Standard, eligible OEM and derived channels use the existing graph/recording
  pipeline through SX, OpenPort and CAN OBD according to actual capabilities.
- UI and MCP show equivalent results. Lock/reconnect/revision changes invalidate
  stale Agent work. Active recordings are not interrupted.
- Existing profiles, recorded sessions and user selections survive migration.

## Delivery order

First deliver generic task recipes, custom preset storage, the top-bar picker,
preview/apply, and MCP parity. Extend the same resolver with exact model/ECU
recipes after their references are checked. The model selection UI ships in the
first slice, even where only generic suggestions are currently available.

Future scope: preset JSON import/export, community recipe packs, multi-ECU rate
budgeting and expanded diagnostic families. No cloud service is required for v1.
