# Scaling the multi-brand OEM research corpus

**Do not equate research volume with working compatibility.** The included ZIP contains a focused source-traceable seed, not a wholesale redistribution of community repositories or licensed SAE specifications.

## Extension procedure

1. Obtain permission, record the license and pin an exact commit or version for each upstream source. The OBDb organization exposes separate vehicle repositories, typically with `signalsets/v3/*.json`. Preserve upstream credits and source URLs. CC BY-SA 4.0 obligations and other repository-specific terms require review before distribution or commercial use.
2. Clone approved repositories into a directory such as `C:\OBDb\<vehicle-repository>\signalsets\v3\...`. The legacy `obdb_bulk_import.py` CSV path converts Service 22 candidates; the v0.3 `deep_sync.py` / `obdb_parse.py` path also accepts source-backed read-only Service 21 candidates. Both paths keep imported OEM definitions inert and disabled for polling.
3. Candidate rows preserve per-configuration filename, model, original field metadata, year filters and source. Some repository filenames or filters may be incomplete; never infer exact ECU applicability from model alone.
4. Require a versioned ECU match: VIN-derived vehicle identity where permitted, ECU address, diagnostic protocol, ECU calibration / software version and supported hardware transport. Validate response lengths and raw frame offsets, source units, signedness, endianness, conditional branches and expected bounds against real or known fixture responses.
5. Promote only the reviewed and tested measurement definition into an independent **installed, versioned, attributed signal pack**. Add regression vectors and record physical-test evidence and device/driver/ECU versions. Unverified definitions stay in research mode with polling disabled; standing authorization may be used for finite source-backed read probes, while state-changing tests require explicit per-test approval under root `AGENTS.md`.
6. Resolve conflicting source definitions conservatively. Keep alternative candidates with independent provenance instead of silently picking one. Allow customers to import their own packs without distributing proprietary datasets from LogNook.

## Namespace rules

- SAE: `SAE_MODE01` and `SAE_MODE09` for traditional legislated diagnostics; newer OBD-on-UDS uses a separate standardized namespace once the applicable licensed annex is verified.
- OEM: `(brand, model, ECU identifier, calibration, protocol, service, DID, subfield)` identifies a diagnostic definition. Reusing a DID on another vehicle does *not* imply identical values.
- Subaru SSM: explicit supported SSM read method + ECU-specific memory offset and conversion, **not** an OBD PID or a UDS `22` DID.
- CAN: network, arbitration ID, ID length, source version, frame layout, multiplexing, bit-order and scaling. Only passively sourced or independently authorized test traffic enters the raw CAN catalog.
- J1939: PGN/SPN and source address, with a licensed implementation of applicable specifications and a separate compatibility roadmap.

## LogNook-specific first acceptance gates

- 2014 Scion xB 2.4L: start with the live Mode 01 map and raw fixtures; exact-model Scion-xB Mode 21/22 rows may be tested as source-backed research candidates at their documented ECU/header. Toyota definitions from other models are lower-confidence leads, not verified xB compatibility. Do not auto-poll unverified candidates.
- 2011 Ford Flex 3.5L EcoBoost: repeat Mode 01 baseline on all compatible adapters, measure latency and data validity. Enhanced *Mustang* definitions are not valid Flex definitions absent independent evidence. Capability-detect available ECU/network access.
- Subaru: identify *the individual car and ECU* before enabling SSM. Keep the RomRaider-derived examples in research mode until licensing and ECU-applicability checks are complete.

Report coverage transparently with separate columns for **documented**, **research candidate**, **bench validated**, **physically verified** and **supported by each adapter**. Never advertise multi-brand support merely by counting unverified catalog rows.
