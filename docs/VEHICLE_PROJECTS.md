# VIN vehicle projects

VIN-capable OBD connections identify the vehicle before selecting a workspace.
The project is a local vehicle profile with a workspace snapshot in
`app_data_dir()/profiles.sqlite`; it is not a separate PID database or a folder
of copied catalog definitions.

## Connection flow

1. Connect an adapter through LogNook's normal Connect control.
2. Read vehicle identity through the existing serialized acquisition owner.
3. A new, unique VIN creates a vehicle project and a basic preset of currently
   supported generic engine signals.
4. A returning VIN offers **Load last state** or **Start fresh**.
5. Load restores eligible saved selections and presentation settings. Start fresh
   uses supported generic defaults. Existing recordings and saved presets remain.

A VIN identifies the vehicle project; it does not establish OEM PID support.
Adapter, ECU, catalog revision and decoder checks still determine what can run.
Unavailable saved selections remain saved rather than being silently discarded.
Simulation and physical vehicle scopes remain separate. VIN-unavailable adapters
continue generic logging and can use manual profile/project selection without
turning manual context into ECU identity evidence.

## Saved state

- Separate logging and graph selections, derived definitions and saved CAN fields.
- Graph preferences, colors, axes and workspace layout.
- Community/Subaru catalog filters and selected preset.

Workspace data is versioned and bounded. Autosave is enabled only for an active
resolved project, never while the reconnect choice is pending. Connection and
project revision checks reject stale writes. Vehicle workspace storage does not
replace the source catalog or erase measurement provenance.

Profile schema v4 adds `vehicle_project_registry` (one profile per scope/VIN) and
`vehicle_project_workspaces` (version 1 JSON, maximum 256 KiB). Existing profiles,
identities, history and custom presets are preserved. When several legacy
profiles share a VIN, migration selects the most recently updated profile as its
canonical project with a deterministic profile-ID tie break; other profiles stay
available. Selection and workspace autosaves share a transaction. Retrying
incomplete initial provisioning repairs its deterministic basic preset without
replacing an existing saved workspace.

Restoring a workspace does not start recording, CAN capture/replay, OEM testing,
or OEM polling. Basic generic measurements can resume after the choice is made.

## Identity details

Offline VIN decoding remains offline and limited to established manufacturer and
model-year evidence. It does not invent model, engine or transmission details.
Saved owner-entered information remains separate from ECU identity evidence.

## Agent Access / MCP

The typed `vehicle_projects` tool uses the same operations and shared service as
the UI: `status`, `list`, `get`, `prepare_on_connect`, `load_last_state`,
`start_fresh`, `save`, and `select_manual`. Generation and expected revision fields
bind mutations to the current connection and saved state. Agent Access must be
unlocked. Preparation uses the current observed identity, not a caller-supplied
VIN. A pending choice is visible in status and does not authorize autosave.

See [VEHICLE_PROFILES.md](VEHICLE_PROFILES.md) for identity evidence, strict profile
matching, history and recording metadata, and [SIGNAL_PRESETS.md](SIGNAL_PRESETS.md)
for preset applicability.

## Verification status (2026-10-19)

Rust core/desktop checks and the packaged-protocol release build passed. Svelte
reported zero errors/warnings; Vite completed with a chunk-size advisory.
Automated tests were not run in this task. The app was opened and the simulator's
no-identity fallback continued supported generic logging and disconnected cleanly.
Scripted simulator diagnostic replies currently lack a responding ECU ID, so
project recognition cannot establish a VIN from them. The positive-VIN project
creation/reconnect prompt, autosave/restore behavior on a vehicle and end-to-end
MCP invocation remain runtime-unverified. No hardware requests were sent.
