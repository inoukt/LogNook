from __future__ import annotations
import argparse, re, sqlite3

def norm(s:str)->str:
    return re.sub(r'[^a-z0-9]+',' ',(s or '').lower()).strip()

def main():
    ap=argparse.ArgumentParser(); ap.add_argument('--db',required=True); ap.add_argument('--replace',action='store_true'); args=ap.parse_args()
    con=sqlite3.connect(args.db); con.row_factory=sqlite3.Row
    if args.replace: con.execute('DELETE FROM candidate_concepts')
    aliases=list(con.execute('''SELECT a.concept_id,a.alias,c.name FROM concept_aliases a JOIN canonical_concepts c USING(concept_id) ORDER BY length(a.alias) DESC'''))
    inserted=0
    for r in con.execute('SELECT candidate_id,name,category,signal_source_id,suggested_metric FROM oem_candidates'):
        hay=norm(' '.join([r['name'] or '',r['category'] or '',r['signal_source_id'] or '',r['suggested_metric'] or '']))
        best={}
        for a in aliases:
            alias=norm(a['alias'])
            if not alias: continue
            if alias in hay:
                score=min(1.0,0.55 + min(len(alias),30)/60)
                cid=a['concept_id']
                if score>best.get(cid,0): best[cid]=score
        for cid,score in sorted(best.items(), key=lambda x:-x[1])[:3]:
            con.execute('INSERT OR REPLACE INTO candidate_concepts(candidate_id,concept_id,match_method,match_score) VALUES(?,?,?,?)',(r['candidate_id'],cid,'alias_heuristic',score)); inserted+=1
    con.commit(); print(f'assigned {inserted} candidate-concept links')
if __name__=='__main__': main()
