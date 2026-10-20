# VIN vehicle projects and workspace restore

## Approved outcome

Connect, read the VIN through the existing serialized identity owner, and open
one local vehicle project per scoped VIN. A new VIN gets a project and basic
supported generic signal preset. A returning VIN prompts **Load last state** or
**Start fresh** before changing the workspace. Keep this flow small and reuse
vehicle profiles and profiles.sqlite.

## Saved workspace

- Logging and graph selections, custom signal definitions and saved references.
- Graph kind, axes, ranges, colors and layout.
- Catalog filters and selected preset; never discovery enablement or running jobs.
- Vehicle-specific saved presets remain available through Start fresh.

## Rules

- VIN establishes the vehicle project, not OEM compatibility. Recheck current
  adapter/ECU capabilities using existing profile resume eligibility.
- Keep simulator and physical vehicle projects separate.
- Pending restore must never autosave the currently visible unrelated workspace
  over the saved project. Scope autosave and load operations to connection
  generation and the currently observed VIN.
- Reconnect, identity conflict and failed reads invalidate stale work. Missing VIN
  leaves normal logging and manual project selection available; manual selection
  does not establish ECU identity.
- Start fresh resets the working workspace, without deleting recordings or saved
  presets. Preserve the previous saved workspace until the fresh workspace is
  deliberately established.
- Do not start recording, OEM discovery, automatic OEM polling, CAN replay,
  diagnostics or CAN capture when restoring state.
- Offline decode supplies only established fields, never guessed model/engine.
- All project data/operations have typed MCP parity through the same core owner
  and Agent Access authorization rules.
- No new database, no edits to the source PID catalog, no source provenance loss.

## Delivery

Use the existing worktree without overwriting existing changes. No commit/push.
Build/type-check the UI and Rust desktop. Physical vehicle behavior must remain
explicitly unverified unless actually observed in this task.
