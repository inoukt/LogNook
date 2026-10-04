"""Offline sanity checks. Run: python tools/test_dataset.py."""
import collections,json,sqlite3,sys,tempfile
from pathlib import Path
ROOT=Path(__file__).resolve().parents[1];sys.path.insert(0,str(ROOT/'tools'))
from obdb_import import parse_commands
from obdb_bulk_import import collect

def check():
    db=ROOT/'data'/'lognook_pids.sqlite';conn=sqlite3.connect(db)
    assert conn.execute('PRAGMA integrity_check').fetchone()[0]=='ok'
    stats=dict(conn.execute('SELECT layer,count(*) FROM signals GROUP BY layer'))
    assert stats['SAE_MODE01']>=170 and stats['OEM_DID']>=70 and stats['SSM_MEMORY']>=30
    assert conn.execute('SELECT COUNT(*) FROM signals WHERE brand != "ALL" AND auto_request != 0').fetchone()[0]==0
    assert conn.execute('SELECT auto_request FROM signals WHERE signal_id="SAE_01_02"').fetchone()[0]==0
    assert conn.execute('SELECT auto_request FROM signals WHERE signal_id="SAE_02_02"').fetchone()[0]==0
    rpm=conn.execute('SELECT formula FROM signals WHERE signal_id="SAE_01_0C"').fetchone()[0]
    assert eval(rpm, {'__builtins__':{}},dict(A=0x1A,B=0xF8))==1726.0
    ect=conn.execute('SELECT formula FROM signals WHERE signal_id="SAE_01_05"').fetchone()[0]
    assert eval(ect,{'__builtins__':{}},dict(A=80))==40
    assert conn.execute('SELECT COUNT(*) FROM signals WHERE layer="SSM_MEMORY" AND identifier!=""').fetchone()[0]==0
    assert conn.execute('SELECT COUNT(*) FROM signals WHERE layer="OEM_DID" AND identifier=""').fetchone()[0]==0
    assert conn.execute('SELECT COUNT(*) FROM subchannels').fetchone()[0]>=75
    cands=parse_commands('Test-Model',{'commands':[{'hdr':'7E0','cmd':{'22':'1234'},'dbgfilter':{'years':[2014]},'signals':[{'id':'X','fmt':{'len':16,'div':10}}]},{'cmd':{'2E':'1234'},'signals':[{'id':'DANGEROUS','fmt':{'len':8}}]}]},'https://example.test')
    assert len(cands)==1 and not cands[0]['auto_request'] and '2014' in cands[0]['year_filter'] and 'dbgfilter' in cands[0]['source_payload']
    # Two source configurations can legitimately share one DID; candidate IDs
    # must preserve their source configuration and all rows remain inert.
    with tempfile.TemporaryDirectory() as tmp:
        sub=Path(tmp)/'Test-Car'/'signalsets'/'v3';sub.mkdir(parents=True)
        sample={'commands':[{'hdr':'7E0','cmd':{'22':'1234'},'filter':{'years':[2011]},'signals':[{'id':'SPD','fmt':{'len':8,'mul':1}}]},{'cmd':{'2E':'1234'},'signals':[{'id':'WRITE','fmt':{'len':8}}]}]}
        (sub/'default.json').write_text(json.dumps(sample),encoding='utf-8')
        sample['commands'][0]['filter']={'years':[2014]}
        (sub/'2014.json').write_text(json.dumps(sample),encoding='utf-8')
        rows,manifest,errors=collect(Path(tmp))
        assert len(rows)==2 and len(manifest)==2 and not errors
        assert len({r['signal_id'] for r in rows})==2
        assert all(r['auto_request'] is False and r['service']=='22' for r in rows)
        assert {'default.json','2014.json'}=={json.loads(r['year_filter'])['source_file'] for r in rows}
    assert len({r[0] for r in conn.execute('SELECT signal_id FROM signals')})==conn.execute('SELECT count(*) FROM signals').fetchone()[0]
    conn.close();print('TESTS PASSED: SQLite, no automatic OEM requests, 2-mode safety, sample SAE formulas, SSM namespace, multichannel, safe single/bulk importers; counts',stats)
if __name__=='__main__':check()
