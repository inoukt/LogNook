# Side quest: OBDLink SX serial research

**Status: completed.** See [the research report](../results/OBDLink-SX-serial-research.md).

**Goal:** Prepare a source-backed integration brief for the next LogNook task: implementing a read-only OBDLink SX adapter behind the existing `HardwareAdapter` contract.

## Read first

- `PROJECT_STATUS.md`
- `LogNook_Blueprint/02_ARCHITECTURE.md`
- `LogNook_Blueprint/03_HARDWARE_PROTOCOLS.md`
- `LogNook_Blueprint/07_REFERENCES.md`
- `crates/lognook-core/src/adapter.rs`

## Work

Use current primary sources from OBDLink/ScanTool and official Windows driver material. Report documented facts separately from field-test questions. Cover:

- USB identity, driver, COM-port setup, and any documented serial settings.
- Safe device/firmware identification and the supported ST/ELM-compatible probe commands.
- Initialization, response framing/termination, echo handling, timeouts, and supported-PID discovery for generic Mode 01.
- Which behaviors vary by firmware and must be capability-probed.
- A read-only test sequence for first bench connection, then stationary generic-OBD validation on the 2014 Scion xB.
- A proposed mapping from documented serial operations into the current `HardwareAdapter` methods.

## Constraints

- Do not edit application source or add dependencies.
- Do not connect to a vehicle, transmit diagnostic commands, change drivers, or test undocumented commands.
- Do not infer baud rate, ST commands, or device compatibility from clone behavior. Mark undocumented or unverified details **unknown**.
- Do not claim Ford enhanced/MS-CAN coverage for the SX.
- Cite each technical claim with a direct primary-source link and the relevant section or page.

## Deliverable

Write `results/OBDLink-SX-serial-research.md` with a concise implementation checklist, a confirmed/unknown table, and a physical-test record template (OS, driver, USB identity, COM port, adapter firmware, vehicle/ECU, PID set, response trace, and measured rate).
