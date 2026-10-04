from pathlib import Path
import json, sqlite3, sys
ROOT=Path(__file__).resolve().parents[1]
sys.path.insert(0,str(ROOT/"tools"))
from obdb_parse import parse_payload, git_blob_sha

def main():
    payload=json.loads((ROOT/"tests/fixture_obdb.json").read_text("utf-8"))
    source={
        "source_id":"TEST_SOURCE","family":"Test Family","brand":"Test",
        "model":"Example","config_file":"signalsets/v3/default.json"
    }
    raw=(ROOT/"tests/fixture_obdb.json").read_bytes()
    rows=parse_payload(source,payload,git_blob_sha(raw))
    assert len(rows)==3, rows
    assert {r["service"] for r in rows}=={"21","22"}
    pressure=next(r for r in rows if r["signal_source_id"]=="TEST_PRESSURE")
    assert pressure["formula"]=="(raw*(1)/(10))+(-5)"
    assert pressure["bit_offset"]==8
    assert pressure["auto_request"]==0
    state=next(r for r in rows if r["signal_source_id"]=="TEST_STATE")
    assert state["value_format"]=="enum"

    db=sqlite3.connect(ROOT/"data/lognook_pids.sqlite")
    curated=db.execute("SELECT COUNT(*) FROM signals").fetchone()[0]
    assert curated==296, curated
    make_sources=db.execute(
        "SELECT COUNT(*),SUM(signal_count) FROM source_corpora WHERE scope='make_fallback'"
    ).fetchone()
    assert make_sources==(34,30585), make_sources
    models=db.execute(
        "SELECT COUNT(*),SUM(signal_count) FROM source_corpora WHERE scope='model_specific'"
    ).fetchone()
    assert models==(24,6510), models
    assert db.execute("SELECT COUNT(*) FROM oem_candidates").fetchone()[0]>0
    invalid_candidates=db.execute(
        "SELECT COUNT(*) FROM oem_candidates WHERE service NOT IN ('21','22') OR auto_request<>0 OR vehicle_validation<>'not_tested'"
    ).fetchone()[0]
    assert invalid_candidates==0, invalid_candidates

    unsafe=db.execute(
        "SELECT COUNT(*) FROM signals WHERE layer IN ('OEM_DID','SSM_MEMORY') AND auto_request<>0"
    ).fetchone()[0]
    assert unsafe==0, unsafe

    flex=db.execute(
        "SELECT signal_count FROM source_corpora WHERE repository='OBDb/Ford-Flex'"
    ).fetchone()[0]
    xb=db.execute(
        "SELECT signal_count FROM source_corpora WHERE repository='OBDb/Scion-xB'"
    ).fetchone()[0]
    assert flex==0 and xb==539, (flex,xb)
    print("PASS: parser, safety rules, source counts, and database integrity")

if __name__=="__main__":
    main()
