# Adapter parity implementation — 2026-10-15

Working tree: `codex/can-event-freeze-frame`, based on
`eda608b3f0c65572e6daac1a76556f4ea5343b23`, with existing local work preserved.
This report describes uncommitted working changes, not a published revision.

## Implemented

- OpenPort standard Mode 06 bitmap and monitor queries use typed requests to
  the existing 32-bit J2534 worker and its bounded receive loop. Matching
  negatives/pending replies and malformed raw evidence remain available;
  unrelated services/identifiers are filtered. Existing Mode 09 uses the shared
  receive helper. Responses must match the requested worker operation.
- OpenPort and Candlelight OBD admit source-backed, catalog-validated OEM
  definitions to the existing continuous acquisition scheduler. UI selection
  supports those adapter families. Transport support does not promote any
  candidate or override vehicle/ECU applicability and validation gates.
- Typed MCP `oem_logging` provides status, catalog search, start and stop. Start
  and stop bind to adapter identity and connection generation. UI/MCP share the
  same plan/application helper. Normal acquisition continues until stopped or
  disconnected; locking Agent Access blocks new commands rather than stopping
  an already started ordinary acquisition plan.
- General `read_vehicle_diagnostics` exposes OpenPort Mode 06. Legacy quick
  `list_mode06_tests`/`try_mode06_test` remain SX/A30-specific.

No driver, firmware, catalog validation records or adapter settings were changed.
No physical adapter/vehicle request was made during this implementation.

## Verification

| Command | Result | Evidence |
| --- | --- | --- |
| `cargo test --workspace --locked -j 4` | 726 passed, zero failed, across 32 targets | [Rust output](adapter-parity-workspace-2026-10-15.txt) |
| `cargo check -p lognook-j2534-worker --target i686-pc-windows-msvc --locked -j 4` | Passed; includes Windows/x86 provider implementation | [Worker output](adapter-parity-worker-check-2026-10-15.txt) |
| `pnpm.cmd run test:ui` in `apps/desktop` | 222 passed, zero failed | [UI tests](adapter-parity-ui-tests-2026-10-15.txt) |
| `pnpm.cmd run build` in `apps/desktop` | Passed; zero Svelte errors/warnings | [UI build](adapter-parity-ui-build-2026-10-15.txt) |
| `cargo fmt --all -- --check` | Passed | Terminal output |
| `git diff --check` | Passed | Terminal output |

One existing UI assertion was tied to the old immediate sample assignment;
it was removed after the earlier frame-batching change. Its surrounding numeric
research graph registration/recording assertions remain.

Release builds use:

```powershell
cargo build --release --locked -p lognook-j2534-worker --target i686-pc-windows-msvc -j 4
cargo build -p lognook-desktop --release --features tauri/custom-protocol --locked -j 4
```

Build evidence: [helper](adapter-parity-helper-build-2026-10-15.txt),
[desktop](adapter-parity-desktop-build-2026-10-15.txt).

Both release builds succeeded. Artifacts under this worktree:

- `target/release/lognook-desktop.exe`: 32,361,984 bytes,
  modified 2026-10-16 02:18:39 UTC (October 15 local time).
- `target/i686-pc-windows-msvc/release/lognook-j2534-worker.exe`: 425,472 bytes,
  modified 2026-10-16 02:14:31 UTC.

The desktop embeds the current built UI using `tauri/custom-protocol`; a local
development server is not required. Tauri bundling remains disabled, so these
are runnable executables and no MSI installer was produced. The app was left
closed after the build.

## Limits and follow-up

Physical Mode 06 and continuous OEM acceptance remain pending on each vehicle,
ECU and adapter combination. Fixture success is not physical compatibility.
The inspected catalog has 37,093 inert, not-tested OEM candidates and zero
validation sessions/results; this change does not make those automatically
eligible for continuous logging. Existing explicit one-shot research remains
separate from validated continuous acquisition.

Graph controls were brainstormed only. The proposed one-bar X/Y zoom strip and
legend-selected Y target are in [the graph note](graph-quick-controls-brainstorm-2026-10-15.md).
No graph control implementation, commit or push was performed.
