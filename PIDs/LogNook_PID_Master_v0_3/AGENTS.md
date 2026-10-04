# AGENTS.md — LogNook Deep PID Database

## Mission

Turn the source registry in this folder into a deep, searchable, versioned automotive diagnostics database for LogNook. The database must favor correct applicability and provenance over inflated PID counts.

This is a production data subsystem, not a demo spreadsheet.

## Immediate task on entering this folder

1. Read `README.md`, `docs/03_SOURCES_LICENSING.md`, and `docs/01_DATA_MODEL_AND_RULES.md`.
2. Run the existing tests before changing anything.
3. If network access is available, populate the SHA-pinned Canada-core sources:

```bash
python tools/deep_sync.py --db data/lognook_pids.sqlite --canada-core --cache-dir cache/obdb --ack-license
python tools/assign_concepts.py --db data/lognook_pids.sqlite --replace
python tests/test_v0_2.py
python tests/test_v0_3.py
```

4. Report the exact number of `oem_candidates` imported, failures, changed upstream SHAs, and per-brand/service counts.
5. Do not call the task complete while `oem_candidates` is empty if network access is available.

## Non-negotiable data rules

- `data/lognook_pids.sqlite` is the authoritative local database.
- Preserve exact upstream request service, identifier/DID, request header, response header, frequency, signal ID, name, category, bit offset, bit length, signedness, scaling, enum mapping, unit, year filters, source payload and Git blob SHA.
- Never convert a researched definition into a universal PID.
- Never infer applicability only from brand name.
- Prefer the most specific compatible definition: exact ECU/config > engine/year/model > model > make fallback.
- Every imported community definition starts with `auto_request=0`, `confidence='research'`, and `vehicle_validation='not_tested'`.
- Physical validation is evidence stored in `validation_sessions` and `validation_results`; do not overwrite research provenance.
- Keep state-changing commands (including DTC clear, reset, erase, coding, service 0x2E, memory writes, routine/output control, actuators, flashing, and security access) out of the measurement catalog, automatic presets, and acquisition queues. This database rule does not prohibit separately authorized physical tests; those follow the root `AGENTS.md` action gate.

## Canadian brand priority

Work deeply, not superficially, on these families:

1. Ford / Lincoln
2. GM: Chevrolet / GMC / Buick / Cadillac
3. Toyota / Lexus / Scion
4. Honda / Acura
5. Hyundai / Kia / Genesis
6. Subaru
7. Nissan / Infiniti
8. Mazda
9. Stellantis: Ram / Jeep / Dodge / Chrysler
10. Volkswagen / Audi
11. Mitsubishi
12. BMW / MINI
13. Mercedes-Benz
14. Volvo
15. Jaguar / Land Rover

Porsche, Fiat, Alfa Romeo and other indexed sources are secondary but should remain supported by the same generic importer.

## Priority physical vehicles

### 2014 Scion xB 2.4L 2AZ-FE

Profile: `SCION_XB_2014_2AZFE`

The pinned xB corpus contains 213 read commands and 539 decoded signals: 107 service-21 commands and 106 service-22 commands. Import the complete corpus. Do not cherry-pick only familiar PIDs.

After import:

```bash
python tools/build_validation_plan.py --db data/lognook_pids.sqlite --vehicle-id SCION_XB_2014_2AZFE --out validation/scion_xb_2014.csv
```

Validate in stages: ECM/PCM first, then TCM, ABS/VSC, TPMS, cluster/body. Use the standing authorization only for exact source-backed read requests; record no-response, negative-response, wrong-length and implausible-decoder outcomes. Any state-changing step requires the root policy's per-test approval gate.

### 2011 Ford Flex 3.5L EcoBoost

Profile: `FORD_FLEX_2011_35EB`

The dedicated Flex corpus is currently empty. Never hide that by labeling F-150/Taurus/Explorer DIDs as Flex PIDs.

Research order:

1. Ford fallback definitions.
2. Taurus definitions because of closer platform/powertrain-era relevance.
3. Explorer definitions.
4. F-150 EcoBoost-era definitions.
5. Only after ECU response and decoder plausibility, promote to Flex-specific validation evidence.

High-value concepts include boost target/actual, TIP/MAP/BARO, wastegate control, charge-air temperature, knock, spark, lambda, HPFP/LPFP, fuel pump command, VCT target/actual, misfires, torque, transmission input/output/converter slip/gear/temp, PTU/AWD, wheel speeds, steering angle, yaw and brake pressure.

### Subaru

Never flatten Subaru SSM memory parameters into ordinary Mode 21/22 PIDs. Keep SSM/SSM2/SSM3/UDS/raw CAN source families distinct. Require model/year/engine/ECU ID before promoting proprietary definitions.

## Canonical concepts

`canonical_concepts` is a UI/search layer, not the decoder source of truth.

Example:

`fuel_pressure_high_actual` may map to different Ford, GM, Toyota, Subaru, VW and Hyundai definitions. Keep each OEM request and formula intact and link them through `candidate_concepts`.

Improve concept mapping conservatively. A wrong canonical link must never change the underlying decoder.

## Import workflow

- Cache upstream JSON in `cache/obdb/`.
- Verify Git blob SHA before parsing.
- If SHA changed, stop that source unless explicitly reviewed with `--allow-updated-sha`.
- Import through `obdb_parse.py`; do not write one-off brand parsers unless the upstream format truly requires it.
- Re-run `assign_concepts.py` after imports.
- Generate a coverage report by family, brand, model, service, module and validation state.
- Keep source files small and reusable. Prefer one generic parser plus data-driven exceptions.

## Completion criteria for the deep-database milestone

Do not call this milestone complete until all are true:

- Canada-core source corpora that are reachable and SHA-valid have been imported.
- `oem_candidates` is populated and contains only services 21/22.
- `auto_request=0` for every unvalidated OEM candidate.
- Scion xB has 539 imported decoded candidates unless the pinned upstream source changed and was explicitly reviewed.
- Ford Flex is accurately represented as having no dedicated upstream definitions, with related-model research clearly separated.
- Per-brand/service/module/model coverage report exists.
- Canonical concepts have been assigned without modifying raw definitions.
- Tests and SQLite integrity check pass.
- Licensing/attribution remains included.

## Code quality

Keep handwritten source files roughly 100–250 lines when practical. Centralize generic parsing, scaling, source verification, vehicle applicability and validation logic. Do not duplicate one importer per manufacturer.
