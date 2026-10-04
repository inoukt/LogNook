# LogNook Deep PID Database v0.3

LogNook's automotive signal catalog is not a flat PID list. It separates standardized diagnostics, researched OEM enhanced definitions, vehicle/model applicability, canonical technician-facing concepts, and physical validation evidence.

## Current package

- SQLite schema version: **3**
- Curated seed definitions: **296**
- SHA-pinned OBDb source corpora: **58**
- Indexed upstream commands: **20,590**
- Indexed upstream decoded signal rows: **37,095**
  - make-level fallback rows: **30,585**
  - model-specific rows: **6,510**
- Imported read-only OEM candidates: **37,093** from **57** SHA-verified source corpora
- canonical diagnostic concepts: **75**
- physical POC vehicle profiles: 2014 Scion xB 2AZ-FE, 2011 Ford Flex 3.5 EcoBoost, Subaru workshop profile

The source registry has 58 corpora; 57 Canada-core corpora are synced at their recorded blob SHAs, including the dedicated Ford Flex corpus which correctly yields zero definitions. The remaining Tesla source has no indexed commands/signals and is outside the Canada-core import. Imported candidates remain inert (`auto_request=0`, `vehicle_validation=not_tested`).

## First command for a network-enabled coding agent

```bash
python tools/deep_sync.py \
  --db data/lognook_pids.sqlite \
  --canada-core \
  --cache-dir cache/obdb \
  --ack-license

python tools/assign_concepts.py \
  --db data/lognook_pids.sqlite \
  --replace

python tests/test_v0_2.py
python tests/test_v0_3.py
```

This imports researched read-only service `21` / `22` definitions for the Canada-core source registry while preserving source payloads and exact blob SHAs. Review upstream changes before using `--allow-updated-sha`. Every imported OEM candidate remains `auto_request=0` and `vehicle_validation=not_tested`.

## Scion xB

The dedicated xB corpus is pinned as `OBDB_SCION_XB_DEFAULT` and currently contains:

- **107 Mode 21 commands / 352 decoded signals**
- **106 Mode 22 commands / 187 decoded signals**
- **213 commands / 539 signals total**

The repository has a `default.json` fallback and no separate 2014 override in the indexed source. Therefore these are **xB research candidates**, not a claim that all 539 are valid on the 2014 2AZ-FE. The generated `validation/scion_xb_2014.csv` includes all 539 candidates; use physical validation before promotion.

After syncing the xB source, create a safe validation list with:

```bash
python tools/build_validation_plan.py \
  --db data/lognook_pids.sqlite \
  --vehicle-id SCION_XB_2014_2AZFE \
  --out validation/scion_xb_2014.csv
```

## Ford Flex

The dedicated `OBDb/Ford-Flex` source currently has zero definitions. The database records that fact explicitly. Do not silently substitute F-150, Explorer, or Taurus definitions.

For research, LogNook indexes:

- Ford make fallback: 404 commands / 485 signals
- Taurus: 180 / 190
- F-150: 116 / 116
- Explorer: 38 / 38

The related-model sources may be useful for discovering EcoBoost-era DIDs, but they must remain related-model research until the Flex ECU responds with a plausible decoder and physical validation confirms it.

## Important tables

- `data/lognook_pids.sqlite` is the authoritative database; the stale top-level `lognook_pids_v0_3.sqlite` is an older seed-only copy and must not be used.
- `signals` — curated seed catalog.
- `oem_candidates` — full imported enhanced definitions. Inert until validated.
- `source_corpora` — exact repositories, SHA pins, counts, scope, licensing and import state.
- `canonical_concepts` — user-facing concepts such as boost, knock, ATF temperature and wheel speed.
- `candidate_concepts` — non-destructive links from OEM definitions to canonical concepts.
- `vehicle_profiles` — exact validation targets.
- `vehicle_supported_commands` — separate user-reported vehicle/ECU command support. The Scion xB 2008–2014 report preserves its 2011 reported year, opaque command codes and subchannel names; it is unverified and disabled for automatic requests.
- `validation_sessions` / `validation_results` — physical evidence.
- `brand_families` / `source_family_aliases` — OEM-family research relationships without claiming cross-brand compatibility.

## Safety and data-integrity rules

1. The deep importer only accepts read-data services `0x21` and `0x22`.
2. No imported OEM definition is automatically approved for polling.
3. A brand fallback is not a model definition.
4. A model definition is not automatically a year/engine/ECU match.
5. Raw source payload, decoder, bit layout, addressing, year filters, SHA and license provenance are retained.
6. Physical responses and decoder plausibility must be recorded separately from community research.
7. Never promote a signal because its name looks correct.
8. The standing project authorization covers source-backed read-only validation probes, not automatic polling of unverified OEM candidates.
9. State-changing commands are not signal definitions and never enter normal acquisition or automatic presets. A separate physical test requires explicit approval for the exact action; flashing/programming remains prohibited.
10. `STPX` is an arbitrary-payload transport wrapper, not a safety classification. Use it only through a constrained, reviewed request path; never expose free-form raw payloads.

See the root `AGENTS.md` for the canonical vehicle-test policy and this package's `AGENTS.md` for catalog-specific rules.
