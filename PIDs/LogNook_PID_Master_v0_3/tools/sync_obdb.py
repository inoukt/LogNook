"""Sync indexed OBDb corpora into LogNook's inert OEM candidate table.

Examples:
  python tools/sync_obdb.py --db data/lognook_pids.sqlite --brand Ford --ack-license
  python tools/sync_obdb.py --db data/lognook_pids.sqlite --scope model_specific --ack-license
  python tools/sync_obdb.py --db data/lognook_pids.sqlite --all --ack-license

Network access is required. Imported rows remain auto_request=0.
"""
from __future__ import annotations
import argparse
import json
import sqlite3
import sys
import urllib.request
from pathlib import Path
from obdb_parse import git_blob_sha, parse_payload

INSERT_SQL = """
INSERT OR REPLACE INTO oem_candidates(
 candidate_id,source_id,family,brand,model_scope,service,identifier,
 request_header,response_header,frequency_hz,category,name,signal_source_id,
 bit_offset,bit_length,value_format,formula,unit,year_filter_json,ecu_filter,
 source_payload,source_blob_sha,research_state,vehicle_validation,auto_request,
 imported_at
) VALUES (
 :candidate_id,:source_id,:family,:brand,:model_scope,:service,:identifier,
 :request_header,:response_header,:frequency_hz,:category,:name,:signal_source_id,
 :bit_offset,:bit_length,:value_format,:formula,:unit,:year_filter_json,:ecu_filter,
 :source_payload,:source_blob_sha,:research_state,:vehicle_validation,:auto_request,
 :imported_at
)
"""

def raw_url(repo: str, config_file: str) -> str:
    path = config_file
    if path.startswith("signalsets/"):
        return f"https://raw.githubusercontent.com/{repo}/main/{path}"
    return f"https://raw.githubusercontent.com/{repo}/main/signalsets/v3/{path}"

def fetch(url: str) -> bytes:
    req = urllib.request.Request(url, headers={"User-Agent": "LogNook-PID-Sync/0.2"})
    with urllib.request.urlopen(req, timeout=30) as response:
        return response.read()

def select_sources(con: sqlite3.Connection, args) -> list[sqlite3.Row]:
    con.row_factory = sqlite3.Row
    where = []
    values = []
    if args.brand:
        where.append("brand = ?")
        values.append(args.brand)
    if args.scope:
        where.append("scope = ?")
        values.append(args.scope)
    if args.source_id:
        where.append("source_id = ?")
        values.append(args.source_id)
    if not (args.all or where):
        raise SystemExit("Choose --all, --brand, --scope, or --source-id.")
    sql = "SELECT * FROM source_corpora"
    if where:
        sql += " WHERE " + " AND ".join(where)
    return list(con.execute(sql + " ORDER BY family,brand,model", values))

def sync_one(con: sqlite3.Connection, source: sqlite3.Row) -> tuple[int, str]:
    url = raw_url(source["repository"], source["config_file"])
    payload_bytes = fetch(url)
    actual_sha = git_blob_sha(payload_bytes)
    expected_sha = source["blob_sha"]
    if expected_sha and actual_sha != expected_sha:
        return 0, f"SHA_CHANGED expected={expected_sha} actual={actual_sha}"
    payload = json.loads(payload_bytes.decode("utf-8"))
    rows = parse_payload(dict(source), payload, actual_sha)

    con.execute("DELETE FROM oem_candidates WHERE source_id = ?", (source["source_id"],))
    con.executemany(INSERT_SQL, rows)
    con.execute(
        "UPDATE source_corpora SET import_state=? WHERE source_id=?",
        (f"synced:{len(rows)}", source["source_id"]),
    )
    return len(rows), "OK"

def main() -> int:
    p = argparse.ArgumentParser()
    p.add_argument("--db", required=True)
    p.add_argument("--all", action="store_true")
    p.add_argument("--brand")
    p.add_argument("--scope", choices=["make_fallback", "model_specific"])
    p.add_argument("--source-id")
    p.add_argument("--ack-license", action="store_true",
                   help="Acknowledge upstream CC BY-SA 4.0 attribution/share-alike requirements.")
    p.add_argument("--allow-updated-sha", action="store_true",
                   help="Refresh changed upstream SHA before import. Review changes first.")
    args = p.parse_args()

    if not args.ack_license:
        raise SystemExit("Pass --ack-license after reviewing docs/03_SOURCES_LICENSING.md.")

    db = Path(args.db)
    con = sqlite3.connect(db)
    con.row_factory = sqlite3.Row
    sources = select_sources(con, args)
    total = 0
    failures = []

    for source in sources:
        try:
            url = raw_url(source["repository"], source["config_file"])
            payload_bytes = fetch(url)
            actual_sha = git_blob_sha(payload_bytes)
            expected_sha = source["blob_sha"]
            if expected_sha and actual_sha != expected_sha:
                if not args.allow_updated_sha:
                    failures.append((source["source_id"], "UPSTREAM_SHA_CHANGED"))
                    print(f"SKIP {source['source_id']}: upstream SHA changed", file=sys.stderr)
                    continue
                con.execute(
                    "UPDATE source_corpora SET blob_sha=?, checked_date=date('now') WHERE source_id=?",
                    (actual_sha, source["source_id"]),
                )
            payload = json.loads(payload_bytes.decode("utf-8"))
            source_dict = dict(source)
            source_dict["blob_sha"] = actual_sha
            rows = parse_payload(source_dict, payload, actual_sha)
            con.execute("DELETE FROM oem_candidates WHERE source_id=?", (source["source_id"],))
            con.executemany(INSERT_SQL, rows)
            con.execute(
                "UPDATE source_corpora SET import_state=? WHERE source_id=?",
                (f"synced:{len(rows)}", source["source_id"]),
            )
            con.commit()
            total += len(rows)
            print(f"{source['source_id']}: {len(rows)} inert candidates")
        except Exception as exc:
            con.rollback()
            failures.append((source["source_id"], str(exc)))
            print(f"ERROR {source['source_id']}: {exc}", file=sys.stderr)

    con.execute(
        "INSERT OR REPLACE INTO db_meta(key,value) VALUES('synced_oem_candidates',?)",
        (str(con.execute("SELECT COUNT(*) FROM oem_candidates").fetchone()[0]),),
    )
    con.commit()
    print(f"Imported {total} rows from {len(sources)} corpora; failures={len(failures)}")
    if failures:
        print(json.dumps(failures, indent=2), file=sys.stderr)
        return 2
    return 0

if __name__ == "__main__":
    raise SystemExit(main())
