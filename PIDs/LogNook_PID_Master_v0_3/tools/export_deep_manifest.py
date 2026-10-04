from __future__ import annotations
import csv,json,sqlite3
from pathlib import Path
ROOT=Path(__file__).resolve().parents[1]; DB=ROOT/'data/lognook_pids.sqlite'
con=sqlite3.connect(DB); con.row_factory=sqlite3.Row
rows=list(con.execute('''SELECT source_id,family,brand,model,scope,repository,config_file,blob_sha,command_count,signal_count,services_json,import_state,research_state,license,source_url FROM source_corpora ORDER BY family,brand,CASE scope WHEN 'make_fallback' THEN 0 ELSE 1 END,model'''))
registry=list(con.execute('SELECT * FROM source_corpora ORDER BY family,brand,CASE scope WHEN \'make_fallback\' THEN 0 ELSE 1 END,model'))
registry_dicts=[dict(r) for r in registry]
with (ROOT/'data/source_corpora.csv').open('w',newline='',encoding='utf-8') as f:
    w=csv.DictWriter(f,fieldnames=registry_dicts[0].keys());w.writeheader();w.writerows(registry_dicts)
(ROOT/'data/source_corpora.json').write_text(json.dumps(registry_dicts,indent=2,ensure_ascii=False)+'\n','utf-8')
with (ROOT/'data/deep_source_manifest.csv').open('w',newline='',encoding='utf-8') as f:
    w=csv.DictWriter(f,fieldnames=rows[0].keys());w.writeheader();w.writerows(dict(r) for r in rows)
core=list(con.execute('''SELECT family,COUNT(*) corpora,SUM(command_count) commands,SUM(signal_count) signals,SUM(CASE WHEN scope='model_specific' THEN signal_count ELSE 0 END) model_signals FROM source_corpora WHERE family IN (SELECT DISTINCT family FROM brand_families WHERE canada_core=1) GROUP BY family ORDER BY signals DESC'''))
with (ROOT/'data/canada_oem_summary.csv').open('w',newline='',encoding='utf-8') as f:
    w=csv.DictWriter(f,fieldnames=core[0].keys());w.writeheader();w.writerows(dict(r) for r in core)
summary={
 'schema_version':3,'curated_signals':con.execute('select count(*) from signals').fetchone()[0],
 'source_corpora':len(rows),'indexed_commands':sum(r['command_count'] for r in rows),'indexed_signals':sum(r['signal_count'] for r in rows),
 'make_fallback_signals':con.execute("select coalesce(sum(signal_count),0) from source_corpora where scope='make_fallback'").fetchone()[0],
 'model_specific_signals':con.execute("select coalesce(sum(signal_count),0) from source_corpora where scope='model_specific'").fetchone()[0],
 'imported_oem_candidates':con.execute('select count(*) from oem_candidates').fetchone()[0],
 'canonical_concepts':con.execute('select count(*) from canonical_concepts').fetchone()[0],
 'vehicle_profiles':con.execute('select count(*) from vehicle_profiles').fetchone()[0],
}
(ROOT/'data/deep_catalog_summary.json').write_text(json.dumps(summary,indent=2)+'\n','utf-8')
catalog_summary={
 'curated_signals':summary['curated_signals'],
 'synced_oem_candidates':summary['imported_oem_candidates'],
 'make_corpora':con.execute("select count(*) from source_corpora where scope='make_fallback'").fetchone()[0],
 'make_signal_rows_indexed':summary['make_fallback_signals'],
 'model_corpora':con.execute("select count(*) from source_corpora where scope='model_specific'").fetchone()[0],
 'model_signal_rows_indexed':summary['model_specific_signals'],
}
(ROOT/'data/catalog_summary.json').write_text(json.dumps(catalog_summary,indent=2)+'\n','utf-8')
print(json.dumps(summary,indent=2))
