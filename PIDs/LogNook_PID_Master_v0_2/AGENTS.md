# LogNook PID Database — Agent Instructions

These instructions apply to this folder and everything below it.

## Mission

Build and maintain LogNook's automotive signal/PID catalog as a **large, searchable, versioned, safety-gated diagnostic database** for Windows-first LogNook development.

The objective is not to collect random PID lists. The objective is to let LogNook resolve the **best applicable signal definition for the connected vehicle/ECU** while preserving source provenance and preventing unverified manufacturer-specific requests from being auto-polled.

Work toward a professional multi-brand diagnostic/tuning logger. Prefer usable implementation and tests over planning documents.

## Read first

Before modifying the database or importer, read:

1. `README.md`
2. `docs/01_DATA_MODEL_AND_RULES.md`
3. `docs/03_SOURCES_LICENSING.md`
4. `docs/06_SOURCE_CORPUS.md`
5. `docs/07_DATABASE_SCHEMA_V2.md`
6. `docs/08_CODEX_DATABASE_TASK.md`

The authoritative development database is:

`data/lognook_pids.sqlite`

Do not create competing PID databases in separate application modules.

## Non-negotiable rules

### 1. Preserve provenance

Every OEM definition must retain enough information to audit where it came from:

- source corpus/source ID
- repository/source URL
- exact Git blob SHA or equivalent source revision
- brand/model scope
- diagnostic service and identifier/DID
- request header and response header/addressing
- decoder/format information
- unit
- year/model/ECU applicability information
- validation state
- raw source payload where available
- licensing/attribution information

Never copy a PID/DID into the database without recording its source.

### 2. Never treat manufacturer-specific PIDs as universal

An OEM DID may vary by:

- model
- model year
- generation
- engine
- transmission
- ECU/module
- ECU calibration/software version
- diagnostic protocol/addressing

Do not infer compatibility from brand alone.

### 3. Imported OEM candidates stay inert

Community/research definitions must enter `oem_candidates` with:

- `auto_request = 0`
- `vehicle_validation = not_tested`
- `research_state = community_reference`
- `ecu_filter = REQUIRES_INDIVIDUAL_ECU_VERIFICATION`

Do not weaken this safety gate to make a demo easier.

Never add an unverified OEM candidate to an automatic logging preset.

### 4. Read-only diagnostic research only

This database project is for measurement/read operations.

The current importer accepts read-oriented services such as `0x21` and `0x22`.

Do not add coding, flashing, actuator-control, security-access, erase, write-memory, write-DID, or other vehicle-changing commands to the automatic PID catalog.

### Research definitions are test leads

This policy applies project-wide to every PID list and source, not only this v0.2 database or its OEM candidate table. See the repository-root `AGENTS.md` for the shared rule.

Catalog entries marked as research candidates are potential definitions to test, not claims that the vehicle supports them and not junk to discard merely because they are unverified. For an explicitly authorized vehicle test, systematically work through the finite, source-backed candidate set for the target make/model/year. If the exact-model corpus is empty, test plausible same-make related-platform entries as low-confidence experiments rather than stopping at the exact-model filter.

Be thorough but bounded: probe only read-only services and identifiers present in a reviewed source or an explicitly scoped test set, use conservative pacing, and stop if the adapter or ECU becomes unstable. Do not sweep arbitrary undocumented identifier space, invoke sessions/security access, or try vehicle-changing services. Record the vehicle and ECU context, protocol/addressing, requested identifier, raw response, decoder hypothesis, observed value/unit, latency, negative response/timeout, and outcome. Keep every candidate out of automatic polling until applicability and decoding are verified.

### 5. Preserve raw measurements

LogNook must preserve original ECU bytes and the exact decoder provenance used at recording time. Unit conversions, formulas, taring and display transformations are derived views; they must not destroy source data.

### 6. Be precise about status

Use explicit states. Do not call a definition "working" merely because it appears in a repository.

Recommended progression:

- `not_tested`
- `responds_unverified_decode`
- `decode_plausible`
- `vehicle_verified`
- `ecu_variant_verified`

Research counts are not verified compatibility counts.

## Definition selection order

When several implementations exist for the same diagnostic concept, prefer the most specific compatible and best-evidenced definition:

1. exact ECU software/calibration + physical verification
2. exact model/year/powertrain + physical verification
3. exact model/year research definition
4. model/generation research definition
5. make/OEM-family fallback
6. standard OBD-II definition when ECU-supported

A lower-specificity definition must never silently override a higher-specificity one.

If compatibility is ambiguous, present the candidate for manual research instead of auto-requesting it.

## Canonical signal concepts

The user should search for a diagnostic concept, not memorize OEM DIDs.

Build toward canonical concepts such as:

- engine RPM
- vehicle speed
- calculated/absolute load
- MAP / manifold pressure
- boost / relative manifold pressure
- barometric pressure
- throttle position / throttle command
- accelerator position
- MAF
- lambda / AFR / equivalence ratio
- STFT / LTFT
- low-side fuel pressure
- rail/high-side fuel pressure
- injector pulse width / injection quantity
- ignition timing
- knock correction / retard / learned knock
- wastegate command / duty / position
- turbo speed
- charge-air temperature
- coolant temperature
- oil temperature / pressure
- intake and exhaust cam angle / VCT command
- misfire counters
- transmission input/output speed
- commanded/actual gear
- transmission temperature
- torque converter slip
- wheel speeds
- steering angle
- AWD/PTU/clutch state
- battery/system voltage
- alternator/generator command
- hybrid/EV SOC, voltage, current, temperatures and cell data
- DPF/SCR/NOx/EGT data
- TPMS pressure/temperature

A canonical concept may map to many OEM implementations. Never store one "universal OEM DID" on the concept.

## Canadian workshop coverage priority

Prioritize broad, useful coverage in this order while continuing to preserve quality gates:

### Tier A

- Ford / Lincoln
- GM: Chevrolet / GMC / Buick / Cadillac
- Toyota / Lexus / Scion
- Honda / Acura
- Hyundai / Genesis / Kia
- Nissan / Infiniti
- Mazda
- Subaru
- Volkswagen / Audi
- Stellantis: Ram / Jeep / Dodge / Chrysler

### Tier B

- Mitsubishi
- BMW / MINI
- Mercedes-Benz
- Volvo
- Porsche
- Jaguar / Land Rover
- Fiat / Alfa Romeo

### Explicitly indexed but sparse

- Tesla: keep indexed, but do not invent diagnostic coverage when the source corpus is empty or inappropriate.

Do not interpret tier as permission to lower validation standards.

## Priority physical test vehicles

These are the project's known POC vehicles and should receive extra tooling, queries and validation support:

### 2011 Ford Flex — 3.5L EcoBoost

Prioritize research around:

- boost/MAP/baro
- throttle
- desired/actual lambda
- STFT/LTFT
- low-pressure fuel system
- high-pressure fuel rail
- injector data
- ignition timing
- knock
- wastegate/turbo control
- VCT/cam positions
- charge-air/intake temperatures
- misfire counters
- transmission data
- PTU/AWD data
- wheel speeds and chassis data where accessible

`Ford-Flex` currently has an empty model-specific upstream default definition. Do **not** silently substitute Taurus, Explorer or F-150 mappings. Related Ford platform definitions are research leads only until validated on the Flex.

### 2014 Scion xB — 2.4L

Prefer the dedicated `Scion-xB` source corpus over generic Toyota fallback when year/applicability matches.

Prioritize:

- fuel trims
- injector data
- lambda/O2
- VVT
- coolant/IAT
- ignition timing
- knock/misfire where available
- transmission data
- charging voltage
- ABS/wheel speeds/TPMS where supported

### Subaru — multiple models

Maintain separate layers for:

- standard OBD
- Subaru enhanced Mode 21/22 candidates
- Subaru SSM definitions

Do not flatten SSM memory addresses into ordinary OBD PID identifiers.

Prioritize tuning-oriented concepts: boost, DAM/learned timing where defined, knock correction, AFR/lambda, fuel trims, MAF, injector data, wastegate duty, VVT, load, ignition timing and relevant CVT/transmission data.

## Working with OBDb source corpora

`source_corpora` is an index. A source row being indexed does not mean all its signals are bundled locally.

Before syncing, review `docs/03_SOURCES_LICENSING.md`.

Typical commands from this folder:

```bash
python tools/query_catalog.py --db data/lognook_pids.sqlite --brand Ford --sources
python tools/query_catalog.py --db data/lognook_pids.sqlite --brand Ford --text boost

python tools/sync_obdb.py \
  --db data/lognook_pids.sqlite \
  --brand Ford \
  --ack-license

python tools/sync_obdb.py \
  --db data/lognook_pids.sqlite \
  --scope model_specific \
  --ack-license
```

The source index is SHA-pinned. If upstream changed, **do not immediately use `--allow-updated-sha`**.

Instead:

1. inspect the upstream change
2. compare affected definitions
3. confirm licensing remains compatible
4. update the indexed SHA intentionally
5. rerun parser/tests
6. document meaningful decoder/applicability changes

## Database editing rules

- Treat `data/lognook_pids.sqlite` as the authoritative catalog artifact.
- Prefer deterministic scripts/migrations over manual SQLite edits.
- Keep source-derived rows immutable where possible; record local validation separately or create a clearly curated/promoted definition retaining provenance.
- Never overwrite upstream raw payload/provenance with a local guess.
- Keep schema changes backward-aware and documented.
- Add indexes only for demonstrated query needs.
- Do not duplicate the whole catalog into JSON/TypeScript/Rust constants.
- Generated CSV/JSON exports are views/distribution artifacts, not separate sources of truth.

If you add a schema migration, update the schema documentation and tests in the same change.

## LogNook application integration

When integrating this database into LogNook:

1. create one catalog service/data-access layer
2. keep PID selection separate from transport and graph UI code
3. query by canonical concept + vehicle/module context
4. resolve the most specific safe definition
5. expose validation/research state in advanced/developer UI
6. cache vehicle-specific supported/verified results using a vehicle fingerprint
7. never mutate source definitions to store vehicle observations
8. preserve decoder/version/source provenance inside recordings
9. keep acquisition scheduling independent from catalog storage
10. do not load the entire database into UI memory

A useful vehicle fingerprint may include, when available:

- VIN or anonymized VIN-derived identity
- make/model/year
- engine/powertrain
- ECU/module address
- calibration ID / CVN / software identifiers
- adapter/protocol used

Do not require every field before generic OBD operation.

## Fast polling considerations

The database should support the acquisition scheduler, not dictate a fixed polling loop.

Keep/request metadata needed for:

- suggested frequency
- batching compatibility
- ECU/module grouping
- protocol/addressing
- expected response size
- signal priority
- slow-changing vs fast-changing channels

Do not assume the source's `freq` value is a guaranteed achievable sampling rate.

Actual rate must be measured on the adapter + vehicle + ECU combination.

## Licensing

OBDb repositories sampled for this package use **CC BY-SA 4.0**. The upstream repository remains authoritative for licensing.

Preserve attribution and source URLs. Do not strip source metadata from redistributed definitions.

Before shipping an OBDb-derived database inside a commercial/distributed LogNook build, review the share-alike implications and packaging strategy. Do not make legal claims beyond the license text/documented review.

## Tests required after changes

At minimum run:

```bash
python tests/test_v0_2.py
```

Also run relevant importer/query smoke tests after modifying parser, schema, source index or query code.

Required invariants include:

- SQLite database opens cleanly
- indexed source counts remain internally consistent
- parser accepts expected read-only definitions
- unsafe/write services are rejected
- imported OEM candidates have `auto_request = 0`
- imported OEM candidates start `not_tested`
- source SHA/provenance is retained
- unverified OEM rows cannot enter automatic presets

If a test cannot be run, state exactly what was not verified.

## Coding style for this database project

- Keep handwritten code files focused and generally small (roughly 100–250 lines where practical).
- Reuse parsing, normalization, source and validation functions; do not copy logic per brand.
- Keep manufacturer quirks in data/adapters, not scattered conditionals across the application.
- Prefer clear data transformations over clever abstractions.
- Keep docs detailed enough to explain decisions; the small-file guideline applies mainly to source code, not documentation.
- Remove generated `__pycache__`, temporary databases and scratch exports from release archives.

## Agent workflow for substantial tasks

For each meaningful change:

1. inspect the current schema/code/data first
2. identify the smallest useful vertical change
3. preserve existing behavior unless intentionally changing it
4. implement the change
5. add/update tests
6. run tests and integrity checks
7. update docs/source metadata if needed
8. report exact counts and what is merely researched vs locally imported vs physically verified

Do not stop at an architecture proposal when implementation is possible.

## Current immediate priorities

1. Sync/import the Canada-core make-level corpora into `oem_candidates` in a controlled manner.
2. Sync useful model-specific corpora, with priority for Scion xB and common Canadian vehicles.
3. Build canonical signal-concept mapping and aliases for fast PID search.
4. Add a reusable definition resolver that scores applicability/specificity without auto-enabling unverified signals.
5. Build vehicle-validation storage separate from source definitions.
6. Build Ford research views for the 2011 Flex EcoBoost and Toyota/Scion views for the 2014 xB.
7. Preserve Subaru SSM as a distinct protocol/definition family.
8. Add tests for selection precedence and validation promotion.
9. Integrate the single catalog service into LogNook's PID picker and presets.

## Definition of success

The database is successful when a technician can connect a vehicle, identify available ECUs, type a concept such as `boost`, `fuel pressure`, `knock`, `VCT`, `trans temp`, or `wheel speed`, and LogNook can quickly show the best applicable definitions with clear confidence/verification state—without blindly polling incompatible OEM requests.
