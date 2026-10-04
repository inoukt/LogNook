# Deep database status — v0.3

## Built locally

- SQLite schema v3
- 296 curated seed definitions
- 58 exact SHA-pinned upstream corpora
- 20,590 indexed upstream commands
- 37,095 indexed decoded signal rows
- 75 canonical technician-facing concepts
- model/family/source provenance tables
- physical vehicle profiles and validation result schema
- generic read-only 0x21/0x22 parser
- SHA-verifying deep sync/cache tool
- canonical concept assignment tool
- read-only vehicle validation-plan generator
- Windows one-command build scripts
- automated v0.2 + v0.3 integrity/safety tests

## Payload import status in this delivered snapshot

`oem_candidates` contains **37,093** read-only candidates imported from **57 of the 58** indexed corpora. The only unsynced source is Tesla, which is outside the Canada-core sync and has zero indexed commands/signals. The 37,095 source-index rows remain a separate upstream count, not the local candidate count.

Every imported row remains `auto_request=0` and `vehicle_validation=not_tested`. SQLite integrity passes, 75 canonical concepts are assigned, and the source registry exports reflect the imported database state.

Three cached upstream files had moved beyond their pinned Git blob SHAs. They were reviewed against the new commits and imported with updated SHA pins. The differences adjust debug/year applicability (including one change from an unqualified debug flag to a year filter); request services, identifiers, ECU headers, signal IDs, and decoder fields are unchanged. Each source still declares CC BY-SA 4.0.

| Source | Prior blob SHA | Imported blob SHA | Reviewed upstream commit |
|---|---|---|---|
| BMW 3 Series | `ea32ae89d01514ea16614c43d612d99f974a0ecd` | `9dc1ed150cf76f5d31f9c8a2d0be0affe9e7eb10` | [e6c61e12](https://github.com/OBDb/BMW-3-Series/commit/e6c61e12df52) |
| Chevrolet Equinox | `e3fde265b4ab9d2c83c4060e085e5998f15f16c5` | `fc0202bf5e1280758c69ecd6a0686a760f3ed3a6` | [655a7a18](https://github.com/OBDb/Chevrolet-Equinox/commit/655a7a18969d) |
| Subaru Outback | `f355308860bf7c1ca77922139dab75db95f996a3` | `48b57f3c21805c0084147aad5ef6b23ea74016a4` | [b763d066](https://github.com/OBDb/Subaru-Outback/commit/b763d0663411) |

## Scion xB status

Pinned source: `OBDB_SCION_XB_DEFAULT`

- 107 service 21 commands / 352 signal rows
- 106 service 22 commands / 187 signal rows
- 213 commands / 539 signal rows total
- All 539 are imported as unvalidated candidates. Service `21 01` has 18 channels at request `7E0`, response `7E8`.

The source is model-specific to xB but is a fallback `default.json`, not a dedicated 2014/2AZ-FE override. Physical validation is therefore required before promotion.

The generated `validation/scion_xb_2014.csv` contains the full 539-row xB read-only validation plan. No physical validation sessions or results are recorded yet.

## Ford Flex status

Pinned dedicated Flex source currently contains 0 commands/signals. Related Ford data is intentionally kept separate:

- Ford fallback: 404 commands / 485 signals
- Taurus: 180 / 190
- F-150: 116 / 116
- Explorer: 38 / 38

Do not label related-model candidates as Flex-specific until tested against the actual 2011 Flex ECU.
