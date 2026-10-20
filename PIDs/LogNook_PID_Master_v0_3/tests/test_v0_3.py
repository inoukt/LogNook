from pathlib import Path
import json,sqlite3,sys,tempfile
ROOT=Path(__file__).resolve().parents[1];sys.path.insert(0,str(ROOT/'tools'))
from obdb_parse import parse_payload,git_blob_sha

def main():
    db=sqlite3.connect(ROOT/'data/lognook_pids.sqlite'); db.row_factory=sqlite3.Row
    assert db.execute('pragma integrity_check').fetchone()[0]=='ok'
    assert db.execute("select value from db_meta where key='schema_version'").fetchone()[0]=='3'
    assert db.execute("select value from db_meta where key='subaru_catalog_schema_version'").fetchone()[0]=='1'
    assert db.execute("select value from db_meta where key='package_version'").fetchone()[0]=='0.3'
    imported=db.execute('select count(*) from oem_candidates').fetchone()[0]
    assert db.execute("select value from db_meta where key='synced_oem_candidates'").fetchone()[0]==str(imported)
    assert db.execute('select count(*) from canonical_concepts').fetchone()[0]>=70
    assert db.execute('select count(*) from vehicle_profiles').fetchone()[0]>=3
    support=db.execute("select model_year_from,model_year_to,reported_model_year,ecu_header,command_code,signal_names_json,auto_request,validation_status from vehicle_supported_commands where profile_id='SCION_XB_2008_2014' and command_code='010C'").fetchone()
    assert tuple(support[:5])==(2008,2014,2011,'7E0','010C')
    assert json.loads(support['signal_names_json'])==['RPM']
    assert support['auto_request']==0 and support['validation_status']=='not_independently_verified'
    assert db.execute("select count(*) from vehicle_supported_commands where profile_id='SCION_XB_2008_2014'").fetchone()[0]==30
    assert db.execute('select count(*) from candidate_concepts').fetchone()[0]>0
    extension=db.execute("select value from db_meta where key='subaru_catalog_schema_version'").fetchone()
    subaru_tables={r[0] for r in db.execute("select name from sqlite_master where type='table' and name like 'subaru_%'")}
    if extension:
        assert extension[0]=='1'
        assert {'subaru_signal_definitions','subaru_signal_ecu_links','subaru_request_hints'} <= subaru_tables
        assert db.execute("select count(*) from subaru_catalog_search where auto_request<>0 or vehicle_validation<>'not_tested'").fetchone()[0]==0
        assert db.execute("select count(*) from subaru_request_hints where mapping_state<>'inferred' or auto_request<>0").fetchone()[0]==0
    else:
        assert not subaru_tables, subaru_tables
    summary=json.loads((ROOT/'data/deep_catalog_summary.json').read_text('utf-8'))
    assert summary['imported_oem_candidates']==imported
    summary=json.loads((ROOT/'data/catalog_summary.json').read_text('utf-8'))
    assert summary['synced_oem_candidates']==imported
    xb=db.execute("select command_count,signal_count,services_json from source_corpora where source_id='OBDB_SCION_XB_DEFAULT'").fetchone()
    assert (xb['command_count'],xb['signal_count'])==(213,539)
    assert json.loads(xb['services_json'])=={'21':107,'22':106}
    scion_services={r['service']:r['n'] for r in db.execute("select service,count(*) n from oem_candidates where source_id='OBDB_SCION_XB_DEFAULT' group by service")}
    assert scion_services=={'21':352,'22':187}
    mazda_odo=[tuple(r) for r in db.execute("select request_header,response_header,count(*) from oem_candidates where source_id='OBDB_MAZDA_DEFAULT' and service='22' and identifier='0103' and signal_source_id='MAZDA_ODO_V2' group by request_header,response_header order by request_header")]
    assert mazda_odo==[('747','74F',1),('7D2','7DA',1)]
    pending={r['source_id'] for r in db.execute("select source_id from source_corpora where import_state not like 'synced:%'")}
    assert pending=={'OBDB_TESLA_DEFAULT'}, pending
    flex=db.execute("select signal_count from source_corpora where source_id='OBDB_FORD_FLEX_DEFAULT'").fetchone()[0]
    assert flex==0
    payload=json.loads((ROOT/'tests/fixture_obdb.json').read_text('utf-8')); raw=(ROOT/'tests/fixture_obdb.json').read_bytes()
    source={'source_id':'TEST_SOURCE','family':'Test Family','brand':'Test','model':'Example','scope':'model_specific','config_file':'signalsets/v3/default.json'}
    rows=parse_payload(source,payload,git_blob_sha(raw)); assert len(rows)==3; assert {r['service'] for r in rows}=={'21','22'}
    assert all(r['auto_request']==0 for r in rows); assert all(r['source_scope']=='model_specific' for r in rows)
    addressed={'commands':[
        {'hdr':'747','rax':'74F','cmd':{'22':'0103'},'signals':[{'id':'ODO','fmt':{'len':24}}]},
        {'hdr':'7D2','rax':'7DA','cmd':{'22':'0103'},'signals':[{'id':'ODO','fmt':{'len':24}}]},
    ]}
    addressed_rows=parse_payload(source,addressed,'a'*40)
    assert len({r['candidate_id'] for r in addressed_rows})==2
    revised_rows=parse_payload(source,addressed,'b'*40)
    assert {r['candidate_id'] for r in addressed_rows}.isdisjoint(r['candidate_id'] for r in revised_rows)
    state=next(r for r in rows if r['signal_source_id']=='TEST_STATE'); assert json.loads(state['decoder_json'])['map']['1']['value']=='ON'
    assert not any(r['service']=='2E' for r in rows)
    unsafe=db.execute("select count(*) from oem_candidates where auto_request<>0 or service not in ('21','22')").fetchone()[0]; assert unsafe==0
    print('PASS v0.3: schema, source exports, xB candidates, address/revision identity, parser, and read-only gates')
if __name__=='__main__':main()
