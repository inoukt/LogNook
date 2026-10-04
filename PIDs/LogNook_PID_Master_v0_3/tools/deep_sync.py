"""Fetch/cache/import SHA-pinned OBDb corpora into LogNook v0.3.

All imports remain inert (auto_request=0). Only read services 0x21/0x22 are accepted by obdb_parse.
Examples:
  python tools/deep_sync.py --db data/lognook_pids.sqlite --canada-core --ack-license
  python tools/deep_sync.py --db data/lognook_pids.sqlite --source-id OBDB_SCION_XB_DEFAULT --ack-license
  python tools/deep_sync.py --db data/lognook_pids.sqlite --offline --cache-dir cache/obdb --all --ack-license
"""
from __future__ import annotations
import argparse, json, sqlite3, sys, urllib.request
from pathlib import Path
from obdb_parse import git_blob_sha, parse_payload

COLS = [
'candidate_id','source_id','family','brand','model_scope','service','identifier','request_header','response_header','frequency_hz','category','name','signal_source_id','bit_offset','bit_length','value_format','formula','unit','year_filter_json','ecu_filter','source_payload','source_blob_sha','research_state','vehicle_validation','auto_request','imported_at','module_type','suggested_metric','decoder_json','min_value','max_value','source_scope','applicability_specificity','confidence','notes']
INSERT='INSERT OR REPLACE INTO oem_candidates('+','.join(COLS)+') VALUES('+','.join(':'+c for c in COLS)+')'
CANADA_FAMILIES={
'Ford Motor Company','General Motors','Toyota Motor Corporation','Honda Motor Company','Hyundai Motor Group','Nissan Motor Corporation','Mazda Motor Corporation','Subaru Corporation','Volkswagen Group','Stellantis','Mitsubishi Motors','BMW Group','Mercedes-Benz Group','Volvo Cars','Jaguar Land Rover'}

def raw_url(repo,config):
    return f'https://raw.githubusercontent.com/{repo}/main/{config}' if str(config).startswith('signalsets/') else f'https://raw.githubusercontent.com/{repo}/main/signalsets/v3/{config}'

def cache_path(root, source):
    safe=source['source_id'].lower()+'.json'; return Path(root)/safe

def fetch(url):
    req=urllib.request.Request(url,headers={'User-Agent':'LogNook-DeepPID/0.3'})
    with urllib.request.urlopen(req,timeout=45) as r:return r.read()

def selected(con,args):
    con.row_factory=sqlite3.Row; wh=[]; vals=[]
    if args.canada_core:
        wh.append('family IN ('+','.join('?' for _ in CANADA_FAMILIES)+')'); vals.extend(sorted(CANADA_FAMILIES))
    if args.brand: wh.append('brand=?'); vals.append(args.brand)
    if args.family: wh.append('family=?'); vals.append(args.family)
    if args.scope: wh.append('scope=?'); vals.append(args.scope)
    if args.source_id: wh.append('source_id=?'); vals.append(args.source_id)
    if not (args.all or wh): raise SystemExit('Choose --all/--canada-core/--brand/--family/--scope/--source-id')
    sql='SELECT * FROM source_corpora'+((' WHERE '+' AND '.join(wh)) if wh else '')+' ORDER BY CASE scope WHEN \'model_specific\' THEN 0 ELSE 1 END,family,brand,model'
    rows=list(con.execute(sql,vals)); return rows[:args.limit] if args.limit else rows

def main():
    ap=argparse.ArgumentParser(); ap.add_argument('--db',required=True); ap.add_argument('--all',action='store_true'); ap.add_argument('--canada-core',action='store_true'); ap.add_argument('--brand'); ap.add_argument('--family'); ap.add_argument('--scope',choices=['make_fallback','model_specific']); ap.add_argument('--source-id'); ap.add_argument('--limit',type=int); ap.add_argument('--cache-dir',default='cache/obdb'); ap.add_argument('--offline',action='store_true'); ap.add_argument('--allow-updated-sha',action='store_true'); ap.add_argument('--ack-license',action='store_true'); args=ap.parse_args()
    if not args.ack_license: raise SystemExit('Review docs/03_SOURCES_LICENSING.md then pass --ack-license.')
    con=sqlite3.connect(args.db); con.row_factory=sqlite3.Row; sources=selected(con,args); Path(args.cache_dir).mkdir(parents=True,exist_ok=True)
    total=0; failures=[]
    for s in sources:
        cp=cache_path(args.cache_dir,s)
        try:
            if cp.exists(): data=cp.read_bytes()
            elif args.offline: raise FileNotFoundError(cp)
            else:
                data=fetch(raw_url(s['repository'],s['config_file'])); cp.write_bytes(data)
            sha=git_blob_sha(data)
            if s['blob_sha'] and sha!=s['blob_sha']:
                if not args.allow_updated_sha: raise RuntimeError(f'SHA_CHANGED expected={s["blob_sha"]} actual={sha}')
                con.execute('UPDATE source_corpora SET blob_sha=?,checked_date=date(\'now\') WHERE source_id=?',(sha,s['source_id']))
            payload=json.loads(data.decode('utf-8')); sd=dict(s); sd['blob_sha']=sha
            rows=parse_payload(sd,payload,sha)
            con.execute('DELETE FROM oem_candidates WHERE source_id=?',(s['source_id'],)); con.executemany(INSERT,rows)
            con.execute('UPDATE source_corpora SET import_state=? WHERE source_id=?',(f'synced:{len(rows)}',s['source_id']))
            con.commit(); total+=len(rows); print(f'{s["source_id"]}: {len(rows)} signals')
        except Exception as e:
            con.rollback(); failures.append((s['source_id'],str(e))); print(f'ERROR {s["source_id"]}: {e}',file=sys.stderr)
    con.execute("INSERT OR REPLACE INTO db_meta(key,value) VALUES('synced_oem_candidates',?)",(str(con.execute('SELECT COUNT(*) FROM oem_candidates').fetchone()[0]),)); con.commit()
    print(json.dumps({'sources':len(sources),'imported_rows':total,'failures':len(failures),'failure_details':failures},indent=2))
    return 2 if failures else 0
if __name__=='__main__': raise SystemExit(main())
