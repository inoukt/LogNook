"""Additive, local-only importer for the Subaru OEM research catalog."""
import argparse
import csv
from datetime import datetime, timezone
import hashlib
import json
from pathlib import Path
import re
import sqlite3

from subaru_catalog_schema import ensure_subaru_schema

ROOT = Path(__file__).resolve().parents[1]
DEFAULT_SOURCE = ROOT.parent / "CatalogV2" / "SubaruCatalog"
DISPLAY_NAME = "Subaru OEM catalog"
ECU_FIELDS = (
    "ecu_id", "ecu_name", "system_short", "component_id", "protocol", "data_rate",
    "ecu_request_id", "tool_response_id", "functional_id", "n_ta", "pid_count",
    "vehicles", "monitor_table", "alias_of",
)
VEHICLE_FIELDS = (
    "id", "label", "models", "model_group", "year", "year_range", "variant",
    "tool", "pub_no", "disp_flag",
)
LINK_ROUTE_FIELDS = (
    "ecu_id", "component_id", "protocol", "data_rate", "ecu_address", "tool_address",
    "functional_address", "n_ta", "max_signal_num",
)
ROW_CONTEXT_FIELDS = {"ecu", "source", "source_table", "alias_of"}


def _json(value):
    return json.dumps(value, sort_keys=True, separators=(",", ":"), ensure_ascii=False)


def _key(prefix, *parts):
    digest = hashlib.sha256("\x1f".join(str(part) for part in parts).encode("utf-8")).hexdigest()
    return f"{prefix}_{digest}"


def _parse_hint(value):
    if not isinstance(value, str):
        return None, None
    parts = value.strip().upper().split()
    if len(parts) != 2 or not re.fullmatch(r"[0-9A-F]+", parts[1]):
        return None, None
    width = {"21": 2, "22": 4}.get(parts[0])
    if width != len(parts[1]):
        return None, None
    return parts[0], parts[1]


def _display_label(value):
    return re.sub(r"\s*>\s*SSM\s*(?:2|3|4|Ⅱ|Ⅲ|Ⅳ)\s*$", "", str(value or "").strip(), flags=re.I)


def _year_scope(vehicle):
    model_year = vehicle.get("year")
    model_year = int(model_year) if isinstance(model_year, (int, float)) else None
    raw_range = vehicle.get("year_range")
    start = end = None
    if vehicle.get("is_pseudo"):
        state = "pseudo"
    elif isinstance(raw_range, list) and len(raw_range) == 2:
        try:
            a, b = map(int, raw_range)
        except (TypeError, ValueError):
            state = "unresolved"
        else:
            if 1900 <= a <= b <= 2100:
                start, end, state = a, b, "source_range_unverified"
            else:
                state = "unresolved"
    elif model_year is not None:
        state = "exact_year"
    else:
        state = "unresolved"
    return model_year, start, end, _json(raw_range) if raw_range is not None else None, state


def _hint_id(source_id, ecu_key, protocol, hint_text):
    return _key("subaru-hint", source_id, ecu_key, protocol, hint_text)


def _insert_many(connection, table, columns, rows):
    if not rows:
        return
    names = ",".join(columns)
    marks = ",".join("?" for _ in columns)
    connection.executemany(f"INSERT OR IGNORE INTO {table}({names}) VALUES({marks})", rows)


def _count(connection, table, source_id):
    return connection.execute(f"SELECT COUNT(*) FROM {table} WHERE source_id=?", (source_id,)).fetchone()[0]


def import_catalog(db_path, source_dir=DEFAULT_SOURCE):
    source_dir = Path(source_dir).resolve()
    catalog_path = source_dir / "catalog.json"
    raw_catalog = catalog_path.read_bytes()
    catalog_sha = hashlib.sha256(raw_catalog).hexdigest()
    catalog = json.loads(raw_catalog.decode("utf-8-sig"))
    if not all(key in catalog for key in ("source", "counts", "vehicles", "ecus")):
        raise ValueError("catalog.json is missing source, counts, vehicles, or ecus")
    revision = str(catalog["source"].get("db_version") or "unknown")
    source_id = f"SUBARU_OEM_{re.sub('[^A-Za-z0-9]+', '_', revision).strip('_')}_{catalog_sha[:16].upper()}"
    try:
        source_path = catalog_path.relative_to(ROOT.parents[1]).as_posix()
    except ValueError:
        source_path = catalog_path.name
    source_metadata = {
        "source": catalog["source"],
        "counts": catalog["counts"],
        "catalog_file": catalog_path.name,
        "catalog_path": source_path,
        "catalog_sha256": catalog_sha,
        "source_url": None,
        "license_status": "not_cleared",
        "raw_source_available": False,
    }
    imported_at = datetime.now(timezone.utc).replace(microsecond=0).isoformat()

    vehicle_rows, ecu_rows = [], []
    vehicle_keys, ecu_keys = {}, {}
    for index, vehicle in enumerate(catalog["vehicles"]):
        vehicle_key = _key("subaru-vehicle", source_id, index)
        vehicle_keys[index] = vehicle_key
        model_year, year_from, year_to, raw_range, range_state = _year_scope(vehicle)
        safe_vehicle = {key: vehicle[key] for key in VEHICLE_FIELDS if key in vehicle}
        vehicle_rows.append((
            vehicle_key, source_id, str(vehicle.get("id", "")), index,
            _display_label(vehicle.get("label")), str(vehicle.get("model_group") or ""),
            _json(vehicle.get("models") or []), model_year, year_from, year_to, raw_range,
            range_state, str(vehicle.get("variant") or ""), int(bool(vehicle.get("is_pseudo"))),
            _json(safe_vehicle),
        ))
    for index, ecu in enumerate(catalog["ecus"]):
        source_ecu_id = str(ecu.get("ecu_id", ""))
        if not source_ecu_id or source_ecu_id in ecu_keys:
            raise ValueError(f"missing or duplicate source ECU id: {source_ecu_id!r}")
        ecu_key = _key("subaru-ecu", source_id, source_ecu_id, index)
        ecu_keys[source_ecu_id] = ecu_key
        safe_ecu = {key: ecu[key] for key in ECU_FIELDS if key in ecu}
        ecu_rows.append((
            ecu_key, source_id, source_ecu_id, index, str(ecu.get("ecu_name") or ""),
            str(ecu.get("system_short") or ""), str(ecu.get("component_id") or ""),
            str(ecu.get("protocol") or "unknown"), ecu.get("data_rate"),
            ecu.get("ecu_request_id"), ecu.get("tool_response_id") or None,
            ecu.get("functional_id") or None, ecu.get("n_ta") or None,
            ecu.get("monitor_table"), str(ecu.get("alias_of") or ""), _json(safe_ecu),
        ))

    vehicle_link_rows = []
    link_ordinal = 0
    for vehicle_index, vehicle in enumerate(catalog["vehicles"]):
        vehicle_key = vehicle_keys[vehicle_index]
        for system in vehicle.get("systems", []):
            for nested_ecu in system.get("ecus", []):
                source_ecu_id = str(nested_ecu.get("ecu_id", ""))
                if source_ecu_id not in ecu_keys:
                    raise ValueError(f"vehicle {vehicle.get('id')} references unknown ECU {source_ecu_id}")
                route_metadata = {key: nested_ecu[key] for key in LINK_ROUTE_FIELDS if key in nested_ecu}
                vehicle_link_rows.append((
                    vehicle_key, ecu_keys[source_ecu_id], str(system.get("system_id") or ""),
                    str(system.get("system") or system.get("short") or ""), link_ordinal,
                    _json(route_metadata),
                ))
                link_ordinal += 1

    hint_rows, definition_rows, signal_link_rows = {}, {}, []
    skipped_non_monitor = 0
    for ecu_index, ecu in enumerate(catalog["ecus"]):
        source_ecu_id = str(ecu["ecu_id"])
        ecu_key = ecu_keys[source_ecu_id]
        protocol = str(ecu.get("protocol") or "unknown")
        for row_ordinal, row in enumerate(ecu.get("pids", [])):
            if row.get("kind") != "data_monitor":
                skipped_non_monitor += 1
                continue
            source_table = str(row.get("source_table") or row.get("source") or ecu.get("monitor_table") or f"unresolved:{source_ecu_id}")
            def_id = _key("subaru-def", source_id, source_table, row_ordinal)
            canonical = {key: value for key, value in row.items() if key not in ROW_CONTEXT_FIELDS}
            if (source_table, row_ordinal) in definition_rows:
                prior = definition_rows[(source_table, row_ordinal)]
                if prior["canonical"] != _json(canonical):
                    raise ValueError(f"conflicting rows for {source_table} ordinal {row_ordinal}")
            else:
                definition_rows[(source_table, row_ordinal)] = {
                    "canonical": _json(canonical),
                    "row": row,
                    "definition_id": def_id,
                    "ecu_index": ecu_index,
                }
            hint_text = str(row.get("request_hint") or "")
            hint_service, hint_identifier = _parse_hint(hint_text)
            hint_id = None
            if hint_text:
                hint_id = _hint_id(source_id, ecu_key, protocol, hint_text)
                hint_rows[(ecu_key, protocol, hint_text)] = (
                    hint_id, source_id, ecu_key, protocol, hint_text,
                    hint_service, hint_identifier, "inferred", 0,
                )
            alias_of = str(ecu.get("alias_of") or row.get("alias_of") or "")
            link_metadata = {
                "ecu": row.get("ecu", source_ecu_id),
                "source": row.get("source", source_table),
                "source_table": row.get("source_table", source_table),
                "alias_of": alias_of,
                "request_hint": row.get("request_hint"),
                "source_row_ordinal": row_ordinal,
            }
            signal_link_rows.append((
                _key("subaru-candidate", source_id, ecu_key, def_id), source_id, def_id,
                ecu_key, hint_id, row_ordinal, source_table, alias_of,
                _json(link_metadata), 0,
            ))

    unique_definitions = []
    for (source_table, row_ordinal), item in sorted(definition_rows.items()):
        row = item["row"]
        unique_definitions.append((
            item["definition_id"], source_id, source_table, row_ordinal, _json(row),
        ))

    expected_rows = catalog["counts"].get("data_monitor_rows")
    expected_definitions = catalog["counts"].get("distinct_rows")
    expanded_count = len(signal_link_rows)
    definition_count = len(unique_definitions)
    if expected_rows is not None and int(expected_rows) != expanded_count:
        raise ValueError(f"catalog row count mismatch: expected {expected_rows}, got {expanded_count}")
    if expected_definitions is not None and int(expected_definitions) != definition_count:
        raise ValueError(f"distinct table-row mismatch: expected {expected_definitions}, got {definition_count}")

    db_path = Path(db_path)
    db_path.parent.mkdir(parents=True, exist_ok=True)
    connection = sqlite3.connect(db_path)
    try:
        ensure_subaru_schema(connection)
        connection.execute("BEGIN IMMEDIATE")
        connection.execute(
            "INSERT OR IGNORE INTO subaru_source_releases VALUES(?,?,?,?,?,?,?,?,?)",
            (source_id, DISPLAY_NAME, revision, catalog_sha, source_path, None, "not_cleared",
             _json(source_metadata), imported_at),
        )
        _insert_many(connection, "subaru_vehicle_specs", (
            "vehicle_key","source_id","source_vehicle_id","source_ordinal","display_label","model_group",
            "models_json","model_year","year_from","year_to","year_range_raw_json","year_range_state",
            "variant","is_pseudo","raw_json"), vehicle_rows)
        _insert_many(connection, "subaru_ecu_definitions", (
            "ecu_key","source_id","source_ecu_id","source_ordinal","ecu_name","system_short","component_id",
            "protocol_family","data_rate","request_header","response_header","functional_header","n_ta",
            "monitor_table","alias_of","raw_metadata_json"), ecu_rows)
        _insert_many(connection, "subaru_vehicle_ecu_links", (
            "vehicle_key","ecu_key","system_id","system_name","source_ordinal","route_metadata_json"), vehicle_link_rows)
        _insert_many(connection, "subaru_signal_definitions", (
            "definition_id","source_id","source_table","source_row_ordinal","raw_row_json"), unique_definitions)
        _insert_many(connection, "subaru_request_hints", (
            "request_hint_id","source_id","ecu_key","protocol_family","hint_text","hint_service","hint_identifier",
            "mapping_state","auto_request"), list(hint_rows.values()))
        _insert_many(connection, "subaru_signal_ecu_links", (
            "candidate_id","source_id","definition_id","ecu_key","request_hint_id","source_row_ordinal",
            "source_table","alias_of","link_metadata_json","auto_request"), signal_link_rows)
        orphan_path = source_dir / "orphan_tables.csv"
        if orphan_path.exists():
            with orphan_path.open(encoding="utf-8-sig", newline="") as stream:
                orphan_rows = [(source_id, row["table"], int(row["rows"]), "unlinked_to_vehicle") for row in csv.DictReader(stream)]
            _insert_many(connection, "subaru_orphan_tables", ("source_id","source_table","row_count","status"), orphan_rows)
        connection.commit()
        return {
            "source_id": source_id,
            "source_display_name": DISPLAY_NAME,
            "source_revision": revision,
            "catalog_sha256": catalog_sha,
            "definition_rows": _count(connection, "subaru_signal_definitions", source_id),
            "signal_ecu_links": _count(connection, "subaru_signal_ecu_links", source_id),
            "request_hints": _count(connection, "subaru_request_hints", source_id),
            "vehicles": _count(connection, "subaru_vehicle_specs", source_id),
            "ecus": _count(connection, "subaru_ecu_definitions", source_id),
            "vehicle_ecu_links": connection.execute(
                "SELECT COUNT(*) FROM subaru_vehicle_ecu_links vl JOIN subaru_vehicle_specs v USING(vehicle_key) WHERE v.source_id=?",
                (source_id,),
            ).fetchone()[0],
            "orphan_tables": _count(connection, "subaru_orphan_tables", source_id),
            "skipped_non_monitor_rows": skipped_non_monitor,
            "auto_request": 0,
        }
    except Exception:
        connection.rollback()
        raise
    finally:
        connection.close()


def main():
    parser = argparse.ArgumentParser(description="Import the local Subaru OEM catalog as inert research data.")
    parser.add_argument("--db", type=Path, default=ROOT / "data" / "lognook_pids.sqlite")
    parser.add_argument("--source-dir", type=Path, default=DEFAULT_SOURCE)
    args = parser.parse_args()
    print(json.dumps(import_catalog(args.db, args.source_dir), indent=2))


if __name__ == "__main__":
    main()
