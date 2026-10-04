"""Query LogNook curated signals, synced candidates, and remote source coverage."""
from __future__ import annotations
import argparse
import sqlite3

def main():
    p=argparse.ArgumentParser()
    p.add_argument("--db",default="data/lognook_pids.sqlite")
    p.add_argument("--brand")
    p.add_argument("--text")
    p.add_argument("--sources",action="store_true")
    p.add_argument("--limit",type=int,default=50)
    a=p.parse_args()
    con=sqlite3.connect(a.db); con.row_factory=sqlite3.Row

    if a.sources:
        sql="SELECT family,brand,model,scope,signal_count,command_count,repository,import_state FROM source_corpora"
        vals=[]; where=[]
        if a.brand: where.append("brand=?"); vals.append(a.brand)
        if where: sql+=" WHERE "+" AND ".join(where)
        sql+=" ORDER BY family,brand,model LIMIT ?"; vals.append(a.limit)
        for r in con.execute(sql,vals): print(dict(r))
        return

    term=(a.text or "").strip()
    vals=[]; clauses=[]
    if a.brand:
        clauses.append("brand=?"); vals.append(a.brand)
    if term:
        clauses.append("(name LIKE ? OR category LIKE ? OR identifier LIKE ?)")
        vals += [f"%{term}%"]*3
    where=(" WHERE "+" AND ".join(clauses)) if clauses else ""

    print("CURATED")
    for r in con.execute(
        "SELECT signal_id,brand,name,service,identifier,unit,research_state,vehicle_validation,auto_request "
        "FROM signals"+where+" ORDER BY brand,name LIMIT ?", vals+[a.limit]):
        print(dict(r))

    print("CANDIDATES")
    for r in con.execute(
        "SELECT candidate_id,brand,model_scope,name,service,identifier,unit,vehicle_validation,auto_request "
        "FROM oem_candidates"+where+" ORDER BY brand,name LIMIT ?", vals+[a.limit]):
        print(dict(r))

if __name__=="__main__": main()
