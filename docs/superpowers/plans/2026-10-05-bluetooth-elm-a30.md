# Bluetooth ELM327 and Xtool A30 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add Bluetooth Classic SPP ELM327 and Xtool A30/A30M adapters, test all captured read-only modes in the initial test path, and graph every numeric source-decoded result immediately with research provenance intact.

**Architecture:** A single Windows RFCOMM byte stream connects paired devices using the standard SPP service-class UUID, allowing Windows SDP to resolve each device's channel. Separate ELM text-protocol and A30 binary-protocol adapters use that stream. Standard continuous samples use the existing `HardwareAdapter` pipeline. Finite OEM/read-mode tests preserve request identity, raw bytes, route, decoder and validation state; successfully decoded numeric channels enter the existing graph sample stream without promotion to vehicle-validated status.

**Tech Stack:** Rust workspace, `windows-sys` Bluetooth/Winsock APIs behind Windows cfg, existing Tauri 2 and Svelte/TypeScript UI, SQLite OEM catalog, Rust fixtures/tests.

**Spec:** `docs/superpowers/specs/2026-10-05-bluetooth-elm-a30-design.md`

## Global Constraints

- Keep acquisition and replay on the shared `HardwareAdapter` / `LogNookEngine` / `Sample` pipeline.
- Keep Bluetooth pairing user-controlled and never require a virtual COM port.
- Use only standard Mode 01 signals and finite captured/source-backed read requests; do not sweep identifiers.
- Preserve request identity separately from decoded channels and retain raw response, responder, source, decoder, and validation state.
- Graph finite numeric decoded values immediately while keeping research/validation metadata unchanged; label unmapped Mode 06 numeric values as raw counts.
- Do not send Mode 04 clear, service 30, FFDA/other writes, routines, security/session changes, or arbitrary raw commands.
- Do not commit or modify the three pre-existing untracked files in `results/`.

## Review Focus

- Classic Bluetooth socket disconnects mid-frame: preserve parser state only while connected; report disconnect and do not emit partial samples.
- A30 responses arrive split or coalesced and may contain several block replies: test arbitrary chunk boundaries and exact XOR/length handling.
- Wrong A30 responder or request echo: retain evidence as a typed outcome and never decode it as the requested signal.
- ELM clones vary in banner, echo, spacing, and null bytes: tolerate common text variants but require positive protocol identity and exact service/PID echo.
- Catalog decoders may return out-of-range, non-finite, or unknown fields: keep raw evidence, graph only finite decoder outputs, and leave validation state unchanged.

## File Map

- Create `crates/lognook-core/src/bluetooth_spp.rs`: paired Classic Bluetooth device discovery and RFCOMM `Read + Write` stream connecting through SPP service UUID `00001101-0000-1000-8000-00805F9B34FB`.
- Create `crates/lognook-core/src/elm327.rs`: ELM prompt protocol and read-only standard OBD adapter.
- Create `crates/lognook-core/src/a30.rs`: A30 checksum codec, session, captured read-mode allowlist, and adapter.
- Modify `crates/lognook-core/src/model.rs`, `engine.rs`, and `pipeline*`: carry source-backed multi-channel research samples without conflating their request IDs or validation status with `SignalDefinition`.
- Modify `crates/lognook-core/src/oem_catalog.rs` only as needed for exact candidate-backed A30/ELM test requests; retain read-only SQLite access.
- Modify `apps/desktop/src-tauri/src/commands.rs`, `commands/`, and `lib.rs`: paired-device listing, identity, connection, and finite read-test commands.
- Modify `apps/desktop/src/connectionControls.ts`, `App.svelte`, `types.ts`, and focused UI components: adapter selection, paired device controls, full read-mode test list, typed result display, and graph output.
- Add Rust tests beside each new core module and integration tests under `crates/lognook-core/tests`; add focused UI tests under `apps/desktop/tests`.
- Update `LogNook_Blueprint/02_ARCHITECTURE.md` and `PROJECT_STATUS.md` after verified behavior is complete.

## Tasks and progress

### 1. Windows Classic Bluetooth RFCOMM transport — complete

- [x] Enumerate paired Classic Bluetooth devices and connect through the SPP service UUID with Windows channel resolution.
- [x] Add address/UUID tests and a clear unsupported-platform error.
- [x] Run workspace tests and Windows desktop build. Physical adapter pairing remains untested.

### 2. ELM327 text adapter — complete

- [x] Implement adapter identification, prompt framing, ELM echo/noise/null handling, supported PID bitmap continuation, exact response echo checks, and standard Mode 01 sample output.
- [x] Add bounded source-backed service 21/22 candidate reads; no arbitrary ELM pass-through or STN command path.
- [x] Cover positive, negative, timeout, and common-pipeline behavior with Rust tests.

### 3. Xtool A30 binary adapter — complete

- [x] Implement captured checksummed framing, Windows SPP session startup, split/coalesced record handling, ECU response decoding, and continuous Mode 01 acquisition.
- [x] Add finite one-shot reads for captured Mode 01, 02, 03, 06, 07, 09, and 0A requests, including Scion TIDs 35/A2/A5.
- [x] Preserve typed response outcomes and raw evidence; do not implement Mode 04, service 30, FFDA, arbitrary writes, routines, security, or session switching.

### 4. Graphable read-test results — complete

- [x] Route finite decoded numeric results through the existing sample pipeline, preserving request identity, ECU, decoder provenance, and research status.
- [x] Graph catalog-backed service 21/22 candidate values immediately; mark out-of-range values `Suspect` without rejecting the trace or changing validation.
- [x] Label unmapped Mode 06 numeric values `raw count`; keep text/DTC/VIN outputs as diagnostics.

### 5. Tauri controls and UI — complete

- [x] Add paired-device refresh, adapter choice, Identify/Connect actions, captured A30 read-test actions, typed outcomes, and evidence display.
- [x] Add focused UI coverage and run UI tests, Svelte checks, and production frontend build.
- [x] Keep all acquired/researched numeric samples on the shared graph pipeline and expose no raw-hex input.

### 6. Documentation and verification — complete; hardware check pending

- [x] Update architecture notes, A30 protocol notes, and project status with implemented behavior and limitations.
- [x] Run `cargo test --workspace -j 1` (**185 passed**), `cargo check --workspace -j 1`, `cargo build -p lognook-desktop --features tauri/custom-protocol -j 1`, `npm run test:ui` (**70 passed**), `npm run build` (0 Svelte errors/warnings), targeted `rustfmt --check`, and `git diff --check`.
- [x] Record build/launch commands and the first hardware smoke test in `PROJECT_STATUS.md`.
- [ ] Pair a physical ELM327 or A30, confirm Identify/Connect, then capture a real supported PID and one source-backed candidate response. No commit or push has been made.
