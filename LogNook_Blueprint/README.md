# LogNook — research-backed engineering blueprint
Research snapshot: 2026-10-02. This blueprint contains product goals, architecture decisions, protocol research and a proposed validation roadmap; it is not the live feature tracker. Some milestones have since shipped or been superseded. For current implementation and hardware evidence, use [Project Status](../PROJECT_STATUS.md). For repository setup and contribution rules, see [CONTRIBUTING.md](../CONTRIBUTING.md) and the [documentation map](../docs/README.md).

## Current implementation

The repository contains a Rust workspace, a Tauri 2 + Svelte/TypeScript desktop app, a shared adapter contract, simulated and hardware OBD acquisition, a bounded sample pipeline, SQLite `.lognook` recording, CSV export/import, session replay, automatic triggers, derived signals, graph preferences, and a read-only searchable OEM candidate catalog. The CAN Workbench handles raw capture, live analysis, and explicit manual CAN operations, including selected-ID replay; Agent MCP exposes supported operations through the same typed app owners. The simulator is explicitly labeled. Adapter/vehicle validation limits and current next steps are documented in project status.

The OEM tab offers the same fifteen quick mode tests on connected OBDLink SX and A30 adapters, including freeze-frame RPM and Mode 06 monitor reads. Its results panel uses the CAN-style drag divider. See [quick OBD mode tests](../docs/DISCOVERY_DIAGNOSTICS.md#run-one-quick-obd-mode-test) for the mode list, graph behavior, and evidence paths.

## OpenPort 2.0 test path

The desktop can list registered 32-bit J2534 04.04 providers named Tactrix, OpenPort, or OPTA, identify one, and connect through the x86 worker. The implementation sends only supported standard Mode 01 reads over ISO 15765 and feeds decoded samples into the regular recorder/graph pipeline. It does not implement raw-CAN monitoring or Toyota-specific Mode 21/22 polling through OpenPort. Live Ford samples were recorded through the OPTA provider; see the [OpenPort hardware report](../results/openport2-opta-read-batch-diagnosis-2026-10-09.md) for observed rates and limits. This does not establish cross-vehicle or long-duration compatibility.

On Windows, install the x86 MSVC Rust target and Visual Studio's x86 MSVC linker/Windows SDK if they are missing. From `apps/desktop`, run:

```powershell
rustup target add i686-pc-windows-msvc
npm run build:j2534-worker
npm run tauri dev
```

In LogNook, select **OpenPort 2**, choose the registered OPTA/Tactrix provider, press **Identify**, then **Connect**. If no provider appears, verify that the matching 32-bit J2534 04.04 provider is registered and its DLL path exists; do not change drivers or automatically switch providers. Follow [current Project Status](../PROJECT_STATUS.md) and the [hardware reports](../docs/hardware/2026-10-02-openport2.md) when planning another run; record provider/API identity, responding ECU, latency, sample rate, and errors. Each vehicle and provider still requires its own validation.

## Product promise
A Windows-first, Linux-ready automotive diagnostic/tuning *measurement* suite: connect quickly, select intelligently, record reliably, analyze deeply, share deliberately. Diagnostic logging is the initial scope, not ECU flashing, bypassing access controls or automated actuator control.

## Non-negotiable principles
1. Acquire accurately before optimizing graphs. Never substitute a displayed sampling rate for measured sample frequency.
2. One canonical `Sample`, `SignalDefinition`, `Marker`, `Source`, `Session`, `Trigger`, and `ConnectionCapability` model; no per-adapter duplication of analysis/recording/UI.
3. Modules own functionality; dependencies flow toward the common engine. No god-objects or pervasive global mutable state.
4. Offline-first, locally persisted, recoverable recordings; network features are optional.
5. Detect capabilities instead of assuming a vendor/product label guarantees them.
6. Each signal has provenance, actual unit, formula version, source ECU, quality and timing.
7. **Keep source-code files focused and compact**, with reusable shared functions and typed boundaries; Markdown research and documentation may be as detailed as necessary. Do not split documentation just to satisfy an arbitrary file-size target.

## Available field-test vehicles and dongles
- **2011 Ford Flex 3.5L EcoBoost:** performance-oriented generic OBD and later validated Ford-enhanced acquisition; note OBDLink SX does **not** provide Ford MS-CAN access.
- **2014 Scion xB 2.4L (2AZ-FE):** generic OBD, Toyota-specific *authorized/validated* profile testing and cross-dongle comparison.
- **Subaru, model/year/ECU dependent:** later SSM and tuning-oriented capture using verified, versioned definitions; do not assume one Subaru profile works on every Subaru.
- Hardware presently reported: **Tactrix OpenPort 2.0, OBDLink SX, generic USB ELM327**. RH-02/CANable remains an initial supported target but physical availability has not been confirmed.
- Detailed field-test matrix: `08_POC_VEHICLES.md`.

## Read in this order
- `01_PRODUCT.md`: user journeys, interface, complete feature inventory.
- `02_ARCHITECTURE.md`: technology decisions, module boundaries and repository layout.
- `03_HARDWARE_PROTOCOLS.md`: four requested interfaces, discovery and standards.
- `04_STORAGE_PERFORMANCE.md`: scheduler, timestamping, data structures and export.
- `05_VALIDATION_ROADMAP.md`: phases, hardware tests, exit criteria and open decisions.
- `06_AI_SHARING_SECURITY.md`: AI, cloud sharing, permissions and remote access.
- `07_REFERENCES.md`: primary sources and unresolved external dependencies.
- `08_POC_VEHICLES.md`: the exact available Ford/Scion field tests, a Subaru track and evidence needed to accept the POC.

Read-only acquisition comes before advanced OEM functions. Integrations must never hold the sampling thread or write to the vehicle implicitly.
