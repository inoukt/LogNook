"""Local standalone search without extra dependencies.

Example: python tools/query_pids.py --brand Subaru --text knock
"""
import argparse,sqlite3
from pathlib import Path
DEFAULT=Path(__file__).resolve().parents[1]/'data'/'lognook_pids.sqlite'

def query(db,brand='',layer='',text='',limit=30):
    sql='SELECT signal_id,brand,layer,service,identifier,memory_offset,name,unit,research_state,vehicle_validation FROM signals WHERE 1=1'
    params=[]
    if brand:
        sql+=' AND brand LIKE ?';params.append('%'+brand+'%')
    if layer:
        sql+=' AND layer = ?';params.append(layer)
    if text:
        sql+=' AND (name LIKE ? OR category LIKE ? OR identifier LIKE ?)';params.extend(['%'+text+'%']*3)
    sql+=' ORDER BY brand,layer,identifier LIMIT ?';params.append(limit)
    with sqlite3.connect(db) as c:return c.execute(sql,params).fetchall()

def main():
    p=argparse.ArgumentParser(description=__doc__)
    p.add_argument('--db',type=Path,default=DEFAULT);p.add_argument('--brand',default='');p.add_argument('--layer',default='');p.add_argument('--text',default='');p.add_argument('--limit',type=int,default=30)
    a=p.parse_args()
    for r in query(a.db,a.brand,a.layer,a.text,max(1,min(a.limit,500))):print(' | '.join(str(x or '-') for x in r))
if __name__=='__main__':main()
