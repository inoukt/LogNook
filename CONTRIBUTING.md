# Contributing to LogNook

Thanks for helping improve LogNook. This guide is the human contributor entry point; [AGENTS.md](AGENTS.md) contains the detailed repository and coding instructions for both people and coding agents.

## Start here

1. Read [README.md](README.md), [current project status](PROJECT_STATUS.md), and [AGENTS.md](AGENTS.md).
2. Read the relevant architecture and feature guide before changing behavior. For MCP work, use [Agent Access](docs/AGENT_ACCESS.md). For the PID catalog, follow the additional rules in [PIDs/AGENTS.md](PIDs/AGENTS.md) and the versioned catalog documentation.
3. Check `git status` before editing. Preserve existing local work and avoid formatting or replacing unrelated changes.
4. Keep changes focused. Add or update tests and user/developer documentation in the same change.

## Repository layout

- `apps/desktop/src`: Svelte and TypeScript user interface.
- `apps/desktop/src-tauri`: Tauri commands, Agent Access/MCP, and the Windows application shell.
- `crates/lognook-core`: shared hardware contracts, acquisition, CAN workbench, decoding, recording, and replay.
- `workers`: isolated architecture-specific hardware helpers, including the x86 J2534 worker.
- `PIDs/LogNook_PID_Master_v0_3`: authoritative OEM research database and its importer/query tools; its source provenance and licensing rules are separate from the app code.
- `docs`: operating guides, hardware evidence, and deferred feature candidates.
- `LogNook_Blueprint`: product goals and architecture/validation references. Use `PROJECT_STATUS.md` for what is implemented now.

## Windows development setup

The primary desktop target is Windows. Install Rust with the MSVC toolchain, Node.js with npm, and Visual Studio Build Tools with the C++ workload and Windows SDK. From the repository root:

```powershell
cargo test --workspace -j 1
cargo check --workspace
```

Then build and run the desktop UI from `apps/desktop`:

```powershell
npm ci
npm run test:ui
npm run build
npm run tauri -- dev
```

OpenPort 2 development additionally needs the 32-bit Rust target and the matching MSVC/Windows SDK tools:

```powershell
rustup target add i686-pc-windows-msvc
npm run build:j2534-worker -- --release
npm run tauri -- build
```

Tauri bundling is disabled in `apps/desktop/src-tauri/tauri.conf.json`; expect a runnable executable rather than an MSI unless bundling is deliberately changed and separately verified. The normal Rust/UI test suites do not require a vehicle or adapter. Keep generated `target`, `node_modules`, and `dist` output out of commits.

## Feature and safety expectations

- Use the existing shared core/service and typed Tauri/IPC boundaries. Do not create a second hardware owner or a parallel recording pipeline.
- **MCP parity is required** for every capability with meaningful data or operations. Expose it in the same change through typed Agent Access/MCP, reuse the same core owner and safety gates, update tool schemas and descriptions, and test discoverability, invocation, results/status, and cancellation/evidence where applicable. For a truly presentation-only feature, record why it has no meaningful MCP equivalent in `PROJECT_STATUS.md`.
- Normal OBD acquisition and polling are read-oriented. CAN Workbench operations that transmit are explicit, local, operator-started actions. Never add automatic bus transmission or run an on-vehicle test without its documented test conditions and authorization.
- OEM definitions need source provenance and remain inert until their exact applicability is validated. Never invent or universalize manufacturer-specific identifiers.
- Preserve raw measurement and protocol evidence; do not claim that a response proves decoder meaning or vehicle compatibility.

## Tests and documentation

Run the tests relevant to the code and finish with `git diff --check`. For app changes, run the Rust workspace tests plus `npm run test:ui` and `npm run build`; state any skipped check and why. Hardware reports must record the exact adapter, vehicle/ECU, conditions, commands, results, and what was not tested. Update `PROJECT_STATUS.md` and architecture/operating docs when behavior changes. Dated plans, specs, and test reports preserve historical decisions; do not treat them as current implementation status without checking the current source and status page.

## License status

This checkout has no repository-wide `LICENSE` file. A project license has not yet been selected, so contributors should not assume the app code is licensed for reuse or redistribution. The OEM PID catalog and any bundled third-party material have separate provenance and license terms; see [the catalog licensing guide](PIDs/LogNook_PID_Master_v0_3/docs/03_SOURCES_LICENSING.md). Ask the project owner to choose the repository license before publishing reuse or redistribution terms.
