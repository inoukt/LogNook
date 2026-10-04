"""Import a locally cloned OBDb vehicle-repository collection as INERT review data.

Usage: python tools/obdb_bulk_import.py /path/to/OBDb-repos output.csv
The directory must contain one subfolder per OBDb vehicle repository.

WARNING: imports remain licensed upstream material; review CC BY-SA attribution,
source repository terms and redistribution before packaging into LogNook.
"""
import csv,json,sys,collections
from pathlib import Path
from urllib.parse import quote
from build_database import FIELDS
from obdb_import import parse_commands

def collect(directory):
    all_rows=[]; manifest=[]; errors=[]
    for repo in sorted(directory.iterdir()):
        if not repo.is_dir():continue
        configs=repo/'signalsets'/'v3'
        if not configs.is_dir():continue
        for path in sorted(configs.glob('*.json')):
            origin=f'https://github.com/OBDb/{quote(repo.name)}/blob/main/signalsets/v3/{quote(path.name)}'
            try:
                payload=json.loads(path.read_text('utf-8'))
                if not isinstance(payload,dict):raise ValueError('Expected configuration JSON object')
                new=parse_commands(repo.name,payload,origin,path.name)
                all_rows.extend(new)
                manifest.append({'repository':repo.name,'config_file':path.name,'imported_candidates':len(new),'source_url':origin,'license_file_present':(repo/'LICENSE').exists(),'year_overrides_require_review':path.name!='default.json'})
            except (ValueError,OSError,UnicodeDecodeError) as exc:errors.append((repo.name,path.name,str(exc)))
    return all_rows,manifest,errors

def main():
    if len(sys.argv)!=3:sys.exit(__doc__)
    directory=Path(sys.argv[1]);dest=Path(sys.argv[2])
    if not directory.is_dir():sys.exit(f'Not a directory: {directory}')
    rows,manifest,errors=collect(directory)
    dest.parent.mkdir(parents=True,exist_ok=True)
    with dest.open('w',encoding='utf-8-sig',newline='') as f:
        writer=csv.DictWriter(f,fieldnames=FIELDS,extrasaction='ignore');writer.writeheader();writer.writerows(rows)
    dest.with_suffix('.manifest.json').write_text(json.dumps({'source_type':'OBDb raw community data; licensing and year/ECU review required','read_only_candidates':len(rows),'configs':manifest,'errors':errors},indent=2),encoding='utf-8')
    print(f'{len(rows)} candidate signals from {len(manifest)} configurations; {len(errors)} errors; zero approved for automatic polling')
    if errors:print('Review first errors:',errors[:5])

if __name__=='__main__':main()
