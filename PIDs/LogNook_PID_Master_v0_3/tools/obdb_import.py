"""Import a locally downloaded OBDb v3 signalset as inert candidate definitions.

Usage: python tools/obdb_import.py Ford-Flex path/to/signalsets/v3/default.json output.csv
Do not ship OBDb-derived outputs without satisfying the source repository license.
"""
import csv, json, re, sys
from pathlib import Path
from urllib.parse import quote
from build_database import FIELDS

def parse_commands(make_model, payload, origin, source_file="default.json"):
    brand = make_model.split('-', 1)[0]
    source_slug=re.sub('[^A-Z0-9]+','_',source_file.upper())[:35].strip('_')
    out=[]
    for command_index, command in enumerate(payload.get('commands', [])):
        requests=command.get('cmd', {})
        if not isinstance(requests, dict):
            continue
        for service,did in requests.items():
            if not re.fullmatch(r'[0-9A-Fa-f]{2}',str(service)) or not re.fullmatch(r'[0-9A-Fa-f]+',str(did)):
                continue
            # By design: ONLY read-only DID 22; all other commands need separate review.
            if str(service).upper() != '22':
                continue
            for index,signal in enumerate(command.get('signals', [])):
                fmt=signal.get('fmt', {}) or {}
                raw_id=signal.get('id') or f'SIGNAL_{command_index}_{index}'
                slug=re.sub('[^A-Z0-9]+','_',str(raw_id).upper())[:88].strip('_')
                bits=fmt.get('len', 0)
                if not isinstance(bits,int) or not (0<bits<=512):
                    continue
                mul,div,add=fmt.get('mul',1),fmt.get('div',1),fmt.get('add',0)
                formula=f'(raw*({mul})/({div}))+({add})' if div else ''
                row={key:'' for key in FIELDS}
                row.update(signal_id=f'OBDB_{make_model.upper().replace("-","_")}_{source_slug}_{service}{did}_{slug}_{index}',layer='OEM_DID',brand=brand,
                    model_filter=make_model,year_filter=json.dumps({'filter':command.get('filter'),'dbgfilter':command.get('dbgfilter'),'source_file':source_file},sort_keys=True),
                    ecu_filter='REQUIRES_INDIVIDUAL_ECU_VERIFICATION',transport='VEHICLE_DEPENDENT',service=service.upper(),identifier=str(did).upper(),memory_offset='',
                    request_header=str(command.get('hdr','')),response_header=str(command.get('rax','')),name=str(signal.get('name',raw_id)),
                    category=str(signal.get('path','OEM')),
                    bytes=(bits+7)//8,bit_offset=fmt.get('bix',0),bit_length=bits,value_format='signed' if fmt.get('sign') else 'uint',formula=formula,
                    unit=str(fmt.get('unit','unknown')),decoder='candidate_raw_bitfield',source_id='OBDB_IMPORTED',source_url=origin,
                    research_state='community_reference',vehicle_validation='not_tested',auto_request=False,
                    notes='Inert candidate: year filters, ECU addressing and payload offsets need independent verification; source_debug='+str(command.get('dbg',False)),
                    source_payload=json.dumps({'command':{k:v for k,v in command.items() if k!='signals'},'signal':signal,'source_file':source_file},separators=(',',':')))
                out.append(row)
    return out

def main():
    if len(sys.argv)!=4:
        sys.exit(__doc__)
    make_model,path,dest=sys.argv[1:]
    origin=f'https://github.com/OBDb/{quote(make_model)}/blob/main/signalsets/v3/{quote(Path(path).name)}'
    rows=parse_commands(make_model,json.loads(Path(path).read_text('utf-8')),origin,Path(path).name)
    with open(dest,'w',newline='',encoding='utf-8') as f:
        writer=csv.DictWriter(f,fieldnames=FIELDS,extrasaction='ignore');writer.writeheader();writer.writerows(rows)
    print(f'{len(rows)} inert candidate signals exported; never auto-request without ECU verification.')

if __name__=='__main__':main()
