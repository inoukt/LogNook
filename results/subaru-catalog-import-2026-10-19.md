# Subaru OEM catalog import report — 2026-10-19

## Imported source and safety state

The local source is displayed as **Subaru OEM catalog**. Its recorded revision is `40.01b`; the local input was `PIDs/CatalogV2/SubaruCatalog/catalog.json`, SHA-256 `7d0b69e88ef68e5e67eb23e8a183a7b1654fb021dd07dcbc3cc42b7a0189135d`. This hash identifies the local export file, not an upstream Git blob. Original vendor/package metadata remains in the database’s internal source metadata JSON.

No source URL or reuse license was supplied. The database records `source_url=NULL` and `license_status='not_cleared'`. Keep the source-derived rows local until rights are reviewed; this import makes no redistribution claim.

All imported request hints have `mapping_state='inferred'`; every signal ECU link has `auto_request=0`; the search view returns `vehicle_validation='not_tested'` and `research_state='review_required'`. No Subaru rows were added to `oem_candidates`. The imported `request_hint` is not communication-support ground truth, so the view is a search/manual-research surface and does not provide an automatic poll plan. Manual transmission remains behind the application’s explicit `can_manual_read` gate.

## Counts

| Imported table or view | Rows |
|---|---:|
| `subaru_source_releases` | 1 |
| `subaru_vehicle_specs` | 98 (96 physical specifications, 2 pseudo records) |
| `subaru_ecu_definitions` | 168 |
| `subaru_vehicle_ecu_links` | 4,086 |
| `subaru_request_hints` | 29,516 |
| `subaru_signal_definitions` | 20,855 |
| `subaru_signal_ecu_links` | 45,364 |
| `subaru_orphan_tables` | 10 |
| `subaru_catalog_search` | 45,364 |

The 20,855 definitions use `(source_table, source_row_ordinal)` identity. All 45,364 ECU relationships are retained; aliases share their source-table row definition while keeping ECU-specific metadata on the link.

The 29,516 inferred hint records divide into 280 service-21 and 29,236 service-22 hints. Across signal ECU links, 2,242 rows carry service-21 hints and 43,122 carry service-22 hints. These are counts of inferred labels, not verified requests. Protocol-family link counts are:

| Protocol family | Linked signal rows |
|---|---:|
| CAN14230 | 768 |
| FHI-CAN | 37,749 |
| KWP14230 | 407 |
| OBD-15765 | 359 |
| Phase4-CAN | 1,067 |
| Phase5-CAN | 4,330 |
| SSM-KWP | 684 |

Vehicle scope covers model-year entries from 2011 through 2025. Four older year ranges contain an invalid `2098` start and are stored as `year_range_state='unresolved'` with no interpreted year bounds. The remaining range is retained as `source_range_unverified`; it is not treated as a validated match. Ten orphan table names and their row counts are retained as notices; no missing decoder rows were invented.

Only `data_monitor` rows from `catalog.json` were imported. The importer does not read active-test, customization, internal-info, or calibration-pack exports. For reference, the supplied companion exports contain 4,568 active-test UI items, 139 customization rows, and 518 pack targets; none are present in the Subaru search tables.

## Database and backup

The base database remains package/schema version `0.3` / `3`; the additive extension is `subaru_catalog_schema_version=1`. `oem_candidates` remains at 37,093 rows.

The original database backup was byte-verified before import:

- Backup: `C:\Users\inouk\LogNook\.git\worktrees\LogNook-can-event-freeze-frame\subaru-backups\lognook_pids.sqlite.pre-subaru-20261019`
- Original and backup size: 57,049,088 bytes
- Original and backup SHA-256: `59bc8957b04e9365ec503a7cc4caaa8017a9c8ce4b101e060ca719abbfd8db36`
- Database size after import: 147,873,792 bytes (+90,824,704 bytes)

The database is a local artifact larger than 100 MB; it was not committed or pushed.

## Verification log

Baseline before implementation:

```text
py -3 tests/test_v0_2.py
PASS: parser, safety rules, source counts, and database integrity

py -3 tests/test_v0_3.py
PASS v0.3: schema, source exports, xB candidates, address/revision identity, parser, and read-only gates
```

After the import:

```text
py -3 tools/import_subaru.py --db data/lognook_pids.sqlite --source-dir ..\CatalogV2\SubaruCatalog
definition_rows=20855
signal_ecu_links=45364
request_hints=29516
vehicles=98
ecus=168
vehicle_ecu_links=4086
orphan_tables=10
skipped_non_monitor_rows=0
auto_request=0

py -3 tests/test_v0_2.py
PASS: parser, safety rules, source counts, and database integrity

py -3 tests/test_v0_3.py
PASS v0.3: schema, source exports, xB candidates, address/revision identity, parser, and read-only gates

py -3 tests/test_subaru_catalog.py
PASS: Subaru normalization, source fidelity, idempotence, scope, and inert request gates

PRAGMA integrity_check
ok

PRAGMA foreign_key_check
0 rows
```

## Files changed for this import

- `PIDs/LogNook_PID_Master_v0_3/tools/subaru_catalog_schema.py`
- `PIDs/LogNook_PID_Master_v0_3/tools/import_subaru.py`
- `PIDs/LogNook_PID_Master_v0_3/tests/test_subaru_catalog.py`
- `PIDs/LogNook_PID_Master_v0_3/tests/test_v0_3.py`
- `PIDs/LogNook_PID_Master_v0_3/README.md`
- `PIDs/LogNook_PID_Master_v0_3/docs/03_SOURCES_LICENSING.md`
- `PIDs/LogNook_PID_Master_v0_3/docs/06_SOURCE_CORPUS.md`
- `PIDs/LogNook_PID_Master_v0_3/docs/07_DATABASE_SCHEMA_V2.md`

Default build scripts remain unchanged: they do not require or automatically
import the owner-supplied pack. Run the importer explicitly for local use.
