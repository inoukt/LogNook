# VIN project workspace implementation plan

**Goal:** Automatically create VIN vehicle projects and offer Load last state /
Start fresh on subsequent connections.

**Spec:** ../specs/2026-10-19-vin-project-workspace-design.md

**Architecture:** Extend existing profiles.sqlite storage and profile resume
validation. Share backend project operations between Tauri and MCP; UI owns
presentation state through a bounded versioned snapshot. Never restore jobs or
transmission enablement.

## Tasks

- [x] Backend: persist one project per scoped VIN with selection/workspace and
  saved basic preset. Typed prepare, load/fresh, autosave/status/list operations;
  stale-generation and identity checks; reuse eligibility for restore.
- [x] Frontend: automatic identity path, compact Load last state / Start fresh
  prompt, debounced ready-only autosave, graph/filter/preset snapshot integration,
  normal generic fallback and manual selection where VIN is unavailable.
- [x] Integration review: current unsupported signals, same VIN with different
  adapter/ECU, conflict/no VIN, reconnect during operations, pending restore
  protection, simulator separation, no restarted jobs, MCP parity.
- [x] Build and document actual behavior/limitations; replace local release only
  when the current app is closed and leave all other work intact.

## Ownership and interface

Backend and frontend agents agree the API contract before dependent edits.
Backend owns Rust/store/MCP; frontend owns TS/Svelte/CSS. Root owns these docs,
integration checks and any follow-up review. No broad cleanup or Git operations.

## Verification

Run UI Svelte/Vite build and Rust checks/release build. Do not run or add automated
tests in this task unless the user requests them (developer instruction).
Review migration/backward compatibility, stale work, request ownership and
autosave transitions explicitly. Record any unverified hardware behavior.
