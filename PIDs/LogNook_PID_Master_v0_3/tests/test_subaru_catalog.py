from pathlib import Path
import json
import sqlite3
import sys
import tempfile
from contextlib import closing

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / "tools"))
from import_subaru import import_catalog


def fixture_catalog():
    def signal(ecu, table, ordinal, hint="22 0001", kind="data_monitor"):
        return {
            "ecu": ecu, "kind": kind, "did": "$0001", "did_bytes": 2,
            "pid_id": str(ordinal + 1), "name": "Engine Speed", "signal_id": "SIG_RPM",
            "conv_method": "2", "byte_length": 2, "byte_pos": 1, "bit_pos": 7,
            "bit_size": 16, "endian": "big", "signed": False, "unit": "rpm",
            "unit_id": "rpm", "lsb_mul": "1", "lsb_div": "4", "offset": "0",
            "significant": "0", "factor": "0.25", "offset_scaled": "0",
            "expression": "raw * 0.25", "min": "0", "max": "8000", "genre": "0",
            "enums": [], "request_hint": hint, "source": table, "source_table": table,
            "alias_of": "",
        }

    base_rows = [signal("100", "100_DTM.xml", 0), signal("100", "100_DTM.xml", 1)]
    alias_rows = [dict(row, ecu="101") for row in base_rows]
    other_rows = [signal("102", "102_DTM.xml", 0, "21 05"),
                  signal("102", "102_DTM.xml", 1, "unparsed")]
    obd_rows = [signal("6002", "6002_DTM.xml", 0)]
    ecus = [
        {"ecu_id": "100", "ecu_name": "Engine", "system_short": "EGI", "component_id": "103",
         "protocol": "FHI-CAN", "data_rate": 500000, "ecu_request_id": "$7E0",
         "tool_response_id": "$7E8", "functional_id": "$7DF", "n_ta": "",
         "pid_count": 2, "vehicles": 1, "monitor_table": "100_DTM.xml", "alias_of": "",
         "security_access": "excluded", "control_payload": "excluded", "calibration_pack": "excluded",
         "pids": base_rows, "other_dids": [{"kind": "active_test", "did": "$4001"}]},
        {"ecu_id": "101", "ecu_name": "Engine Copy", "system_short": "EGI", "component_id": "103",
         "protocol": "FHI-CAN", "data_rate": 500000, "ecu_request_id": "$7E1",
         "tool_response_id": "$7E9", "functional_id": "", "n_ta": "",
         "pid_count": 2, "vehicles": 1, "monitor_table": "100_DTM.xml", "alias_of": "100",
         "pids": alias_rows, "other_dids": []},
        {"ecu_id": "102", "ecu_name": "Older Engine", "system_short": "EGI", "component_id": "103",
         "protocol": "SSM-KWP", "data_rate": 4800, "ecu_request_id": "$10",
         "tool_response_id": "$11", "functional_id": "", "n_ta": "",
         "pid_count": 2, "vehicles": 1, "monitor_table": "102_DTM.xml", "alias_of": "",
         "pids": other_rows, "other_dids": []},
        {"ecu_id": "6002", "ecu_name": "OBD", "system_short": "OBD", "component_id": "301",
         "protocol": "OBD-15765", "data_rate": 500000, "ecu_request_id": "$7DF",
         "tool_response_id": "", "functional_id": "$7DF", "n_ta": "",
         "pid_count": 1, "vehicles": 1, "monitor_table": "6002_DTM.xml", "alias_of": "",
         "pids": obd_rows, "other_dids": []},
    ]
    return {
        "source": {"product": "Private source product", "installer": "source.exe",
                   "db_version": "40.01b", "word_db": "M_English.xml", "extracted_by": "build_catalog.py"},
        "scaling_note": "value = raw * scale",
        "counts": {"vehicles": 2, "ecus": 4, "data_monitor_rows": 7,
                   "distinct_rows": 5, "duplicate_rows": 2, "orphan_tables": 1,
                   "orphan_rows": 1},
        "vehicles": [
            {"is_pseudo": False, "id": "0", "label": "Legacy > 14MY > SSM4", "models": ["Legacy"],
             "model_group": "Legacy", "year": 2014, "year_range": [2098, 2014], "variant": "",
             "tool": "", "pub_no": "", "systems": [{"system_id": "100", "system": "Engine",
              "ecus": [{"ecu_id": "100"}, {"ecu_id": "101"}, {"ecu_id": "102"}]}]},
            {"is_pseudo": True, "id": "9999", "label": "OBD", "models": ["Common"],
             "model_group": "Common", "year": None, "year_range": None, "variant": "",
             "tool": "", "pub_no": "", "systems": [{"system_id": "9999", "system": "OBD",
              "ecus": [{"ecu_id": "6002"}]}]},
        ],
        "ecus": ecus,
    }


def main():
    with tempfile.TemporaryDirectory() as tmp:
        root = Path(tmp)
        source = root / "source"
        source.mkdir()
        (source / "catalog.json").write_text(json.dumps(fixture_catalog()), encoding="utf-8")
        (source / "orphan_tables.csv").write_text("table,rows\nmissing_DTM.xml,1\n", encoding="utf-8")
        db_path = root / "catalog.sqlite"
        with closing(sqlite3.connect(db_path)) as db:
            db.execute("CREATE TABLE db_meta(key TEXT PRIMARY KEY, value TEXT NOT NULL)")
            db.execute("INSERT INTO db_meta VALUES('schema_version','3')")
            db.execute("INSERT INTO db_meta VALUES('package_version','0.3')")
            db.commit()

        result = import_catalog(db_path, source)
        assert result["definition_rows"] == 5, result
        assert result["signal_ecu_links"] == 7, result
        assert result["request_hints"] == 5, result
        assert result["orphan_tables"] == 1, result

        with closing(sqlite3.connect(db_path)) as db:
            db.row_factory = sqlite3.Row
            assert db.execute("SELECT value FROM db_meta WHERE key='schema_version'").fetchone()[0] == "3"
            assert db.execute("SELECT value FROM db_meta WHERE key='subaru_catalog_schema_version'").fetchone()[0] == "1"
            assert db.execute("SELECT COUNT(*) FROM subaru_signal_definitions").fetchone()[0] == 5
            assert db.execute("SELECT COUNT(*) FROM subaru_signal_ecu_links").fetchone()[0] == 7
            assert db.execute("SELECT COUNT(*) FROM subaru_vehicle_ecu_links").fetchone()[0] == 4
            assert db.execute("SELECT row_count FROM subaru_orphan_tables").fetchone()[0] == 1
            assert db.execute("SELECT COUNT(*) FROM subaru_signal_ecu_links WHERE auto_request<>0").fetchone()[0] == 0
            assert db.execute("SELECT COUNT(*) FROM subaru_request_hints WHERE mapping_state<>'inferred' OR auto_request<>0").fetchone()[0] == 0
            assert db.execute("SELECT COUNT(*) FROM subaru_signal_ecu_links WHERE request_hint_id IS NULL").fetchone()[0] == 0

            rows = db.execute("SELECT * FROM subaru_catalog_search ORDER BY ecu_id, source_row_ordinal").fetchall()
            assert len(rows) == 7
            assert {row["protocol_family"] for row in rows} == {"FHI-CAN", "SSM-KWP", "OBD-15765"}
            assert all(row["vehicle_validation"] == "not_tested" for row in rows)
            assert all(row["auto_request"] == 0 for row in rows)
            assert all(row["mapping_state"] == "inferred" for row in rows)
            assert "Private source product" not in rows[0]["source_display_name"]
            metadata = db.execute("SELECT source_metadata_json,license_status,source_url FROM subaru_source_releases").fetchone()
            assert "Private source product" in metadata["source_metadata_json"]
            assert metadata["license_status"] == "not_cleared" and metadata["source_url"] is None
            ecu_raw = db.execute("SELECT raw_metadata_json FROM subaru_ecu_definitions WHERE source_ecu_id='100'").fetchone()[0]
            assert "security_access" not in ecu_raw and "control_payload" not in ecu_raw and "calibration_pack" not in ecu_raw
            assert rows[0]["raw_row_json"] and rows[0]["link_metadata_json"]

            alias_pair = db.execute(
                "SELECT definition_id FROM subaru_catalog_search WHERE ecu_id IN ('100','101') AND source_row_ordinal=0"
            ).fetchall()
            assert len(alias_pair) == 2 and alias_pair[0][0] == alias_pair[1][0]
            other = db.execute("SELECT hint_service,hint_identifier,mapping_state FROM subaru_catalog_search WHERE request_hint='unparsed'").fetchone()
            assert tuple(other) == (None, None, "inferred")

            vehicle = db.execute("SELECT * FROM subaru_vehicle_specs WHERE source_vehicle_id='0'").fetchone()
            assert vehicle["year_range_state"] == "unresolved"
            assert vehicle["year_from"] is None and vehicle["year_to"] is None
            assert "Legacy > 14MY" in vehicle["display_label"]
            assert "SSM4" not in vehicle["display_label"]
            scope = json.loads(rows[0]["vehicle_scope_json"])
            assert scope and scope[0]["year_range_state"] == "unresolved"
            assert db.execute("SELECT COUNT(*) FROM subaru_catalog_search WHERE name LIKE '%Engine Speed%'").fetchone()[0] == 7
            assert db.execute("SELECT COUNT(*) FROM subaru_catalog_search WHERE name LIKE '%active_test%'").fetchone()[0] == 0

        rerun = import_catalog(db_path, source)
        assert rerun["definition_rows"] == 5 and rerun["signal_ecu_links"] == 7
        with closing(sqlite3.connect(db_path)) as db:
            assert db.execute("SELECT COUNT(*) FROM subaru_source_releases").fetchone()[0] == 1
            assert db.execute("SELECT COUNT(*) FROM subaru_signal_definitions").fetchone()[0] == 5
            assert db.execute("SELECT COUNT(*) FROM subaru_signal_ecu_links").fetchone()[0] == 7
            assert db.execute("PRAGMA integrity_check").fetchone()[0] == "ok"
    print("PASS: Subaru normalization, source fidelity, idempotence, scope, and inert request gates")


if __name__ == "__main__":
    main()
