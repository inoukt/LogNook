"""Build a READ-ONLY candidate validation plan for one known vehicle profile."""
from __future__ import annotations
import argparse,csv,json,sqlite3
from pathlib import Path

def main():
    ap=argparse.ArgumentParser(); ap.add_argument('--db',required=True); ap.add_argument('--vehicle-id',required=True); ap.add_argument('--out',required=True); ap.add_argument('--include-family-fallback',action='store_true'); ap.add_argument('--include-related-models',action='store_true'); args=ap.parse_args()
    con=sqlite3.connect(args.db); con.row_factory=sqlite3.Row
    v=con.execute('SELECT * FROM vehicle_profiles WHERE vehicle_id=?',(args.vehicle_id,)).fetchone()
    if not v: raise SystemExit('Unknown vehicle profile')
    family=con.execute('SELECT family FROM brand_families WHERE brand=?',(v['brand'],)).fetchone(); family=family[0] if family else ''
    clauses=['(brand=? AND model_scope=?)']; vals=[v['brand'],v['model']]
    if args.include_family_fallback:
        clauses.append('(family=? AND model_scope=\'\')'); vals.append(family)
    if args.include_related_models:
        clauses.append('(family=? AND model_scope<>\'\')'); vals.append(family)
    sql='''SELECT candidate_id,source_id,family,brand,model_scope,service,identifier,request_header,response_header,frequency_hz,module_type,category,name,unit,bit_offset,bit_length,formula,decoder_json,year_filter_json,applicability_specificity,confidence FROM oem_candidates WHERE service IN ('21','22') AND ('''+ ' OR '.join(clauses)+') ORDER BY applicability_specificity DESC,model_scope,name'
    rows=list(con.execute(sql,vals)); out=Path(args.out); out.parent.mkdir(parents=True,exist_ok=True)
    with out.open('w',newline='',encoding='utf-8') as f:
        w=csv.DictWriter(f,fieldnames=['candidate_id','source_id','family','brand','model_scope','service','identifier','request_header','response_header','frequency_hz','module_type','category','name','unit','bit_offset','bit_length','formula','decoder_json','year_filter_json','applicability_specificity','confidence','test_state']); w.writeheader()
        for r in rows: d=dict(r); d['test_state']='UNTESTED'; w.writerow(d)
    print(json.dumps({'vehicle':args.vehicle_id,'read_only_candidates':len(rows),'output':str(out)},indent=2))
if __name__=='__main__':main()
