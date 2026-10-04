"""Create reproducible SQLite/CSV/JSON releases from curated inert references."""
import csv,json,sqlite3,collections
from pathlib import Path
from standard_seed import records as std_records, SERVICE09
from oem_seed import records as oem_records
ROOT=Path(__file__).resolve().parents[1]
DATA=ROOT/'data';DATA.mkdir(exist_ok=True)
FIELDS='signal_id layer brand model_filter year_filter ecu_filter transport service identifier memory_offset request_header response_header name category bytes bit_offset bit_length value_format formula unit decoder source_id source_url research_state vehicle_validation auto_request notes source_payload'.split()
SAE_URL='https://en.wikipedia.org/wiki/OBD-II_PIDs'

def mode09_rows():
    out=[]
    for pid,(name,size,unit) in SERVICE09.items():
        out.append(dict(signal_id='SAE_09_'+pid,layer='SAE_MODE09',brand='ALL',model_filter='',year_filter='',ecu_filter='',transport='OBD_GENERIC',service='09',identifier=pid,memory_offset='',request_header='',response_header='',name=name,category='Identity',bytes=size,bit_offset='',bit_length='',value_format=unit,formula='',unit=unit,decoder='response_parser_required',source_id='WIKI_OBD_PUBLIC',source_url=SAE_URL,research_state='public_reference',vehicle_validation='not_tested',auto_request=(pid=='00'),notes='Variable-length transport responses require dedicated parsers; VIN/CALID/ECU names only with user consent and policy.'))
    return out

def channel_rows(master):
    out=[]
    for m in master:
        if m['layer']!='SAE_MODE01':continue
        p=int(m['identifier'],16)
        pieces=[]
        if 0x14<=p<=0x1B:
            pieces=[('voltage','V','A/200','A'),('short_fuel_trim','%','(B-128)*100/128','B != 255')]
        elif 0x24<=p<=0x2B:
            pieces=[('lambda','lambda','(A*256+B)*2/65536',''),('oxygen_voltage','V','(C*256+D)*8/65536','')]
        elif 0x34<=p<=0x3B:
            pieces=[('lambda','lambda','(A*256+B)*2/65536',''),('oxygen_current','mA','(C*256+D)/256-128','')]
        elif p==0x64:
            pieces=[(x,'% ',f'{letter}-125','') for x,letter in [('idle','A'),('point_1','B'),('point_2','C'),('point_3','D'),('point_4','E')]]
        elif p==0x66:
            pieces=[('maf_a','g/s','(B*256+C)/32','A bit0 supported'),('maf_b','g/s','(D*256+E)/32','A bit1 supported')]
        elif p in (0x67,0x68):
            pieces=[('sensor_1','°C','B-40','A bit0 supported'),('sensor_2','°C','C-40','A bit1 supported')]
        elif p in (0x78,0x79):
            for i,(hi,lo) in enumerate([('B','C'),('D','E'),('F','G'),('H','I')],1):
                pieces.append((f'egt_{i}','°C',f'({hi}*256+{lo})/10-40',f'A bit{i-1} supported'))
        elif p in (0x55,0x56,0x57,0x58):
            pieces=[('channel_a','%','A*100/128-100',''),('channel_b','%','B*100/128-100','')]
        elif p==0xA4:pieces=[('gear_ratio','ratio','(C*256+D)/1000','A bit1 supported')]
        elif p==0xA5:pieces=[('dosing','%','B/2','A bit0 supported')]
        elif p==0x4F:pieces=[('max_lambda','lambda','A',''),('max_o2_volts','V','B',''),('max_o2_current','mA','C',''),('max_map','kPa','D*10','')]
        for part,unit,formula,guard in pieces:
            out.append(dict(channel_id=m['signal_id']+'_'+part,signal_id=m['signal_id'],name=part,formula=formula,unit=unit,availability_rule=guard,verification='researched_decoder_review'))
    return out

def write_csv(path,rows,fieldnames):
    with path.open('w',newline='',encoding='utf-8-sig') as f:
        writer=csv.DictWriter(f,fieldnames=fieldnames,extrasaction='ignore');writer.writeheader();writer.writerows(rows)

def build():
    master=std_records()+mode09_rows()+oem_records()
    master.append(dict(signal_id='SAE_02_02',layer='SAE_MODE02',brand='ALL',model_filter='',year_filter='',ecu_filter='',transport='OBD_GENERIC',service='02',identifier='02',memory_offset='',request_header='',response_header='',name='Freeze frame DTC triggering stored frame',category='Diagnostics',bytes=2,bit_offset='',bit_length='',value_format='DTC',formula='',unit='DTC',decoder='frame_specific_decoder_required',source_id='WIKI_OBD_PUBLIC',source_url=SAE_URL,research_state='public_reference',vehicle_validation='not_tested',auto_request=False,notes='Requires selected freeze frame number; other Mode 02 PIDs are contextual templates of supported Mode 01 fields.'))
    keys=[r['signal_id'] for r in master]
    if len(keys)!=len(set(keys)):
        raise ValueError('Duplicate IDs: '+repr([i for i,n in collections.Counter(keys).items() if n>1]))
    for row in master:
        for field in FIELDS:row.setdefault(field,'')
    channels=channel_rows(master)
    write_csv(DATA/'signals_master.csv',master,FIELDS)
    write_csv(DATA/'decoded_subchannels.csv',channels,['channel_id','signal_id','name','formula','unit','availability_rule','verification'])
    (DATA/'signals_master.json').write_text(json.dumps(master,ensure_ascii=False,indent=2),encoding='utf-8')
    db=DATA/'lognook_pids.sqlite'; db.unlink(missing_ok=True)
    with sqlite3.connect(db) as conn:
        columns=','.join('"'+f+'" '+('INTEGER' if f in ('bytes','bit_offset','bit_length','auto_request') else 'TEXT') for f in FIELDS)
        conn.execute(f'CREATE TABLE signals ({columns},PRIMARY KEY(signal_id))')
        place=','.join('?' for _ in FIELDS)
        conn.executemany(f'INSERT INTO signals VALUES({place})',[[int(r[f]) if f=='auto_request' else r[f] for f in FIELDS] for r in master])
        conn.execute('CREATE INDEX ix_search ON signals(layer,brand,service,identifier)')
        conn.execute('CREATE INDEX ix_validation ON signals(vehicle_validation,auto_request)')
        conn.execute('CREATE TABLE subchannels (channel_id TEXT PRIMARY KEY,signal_id TEXT,name TEXT,formula TEXT,unit TEXT,availability_rule TEXT,verification TEXT,FOREIGN KEY(signal_id) REFERENCES signals(signal_id))')
        conn.executemany('INSERT INTO subchannels VALUES(?,?,?,?,?,?,?)',[tuple(r.values()) for r in channels]);conn.execute('PRAGMA foreign_keys=ON')
        try:
            conn.execute('CREATE VIRTUAL TABLE signal_search USING fts5(signal_id,name,category,brand,content=signals,content_rowid=rowid)')
            conn.execute('INSERT INTO signal_search(signal_search) VALUES ("rebuild")')
        except sqlite3.OperationalError:pass
        assert conn.execute('SELECT COUNT(*) FROM signals').fetchone()[0]==len(master)
        assert conn.execute('PRAGMA quick_check').fetchone()[0]=='ok'
    counts=dict(collections.Counter(r['layer'] for r in master))
    (DATA/'release_stats.json').write_text(json.dumps(dict(version='0.1',as_of='2026-10-02',counts=counts,total=len(master),subchannels=len(channels),vehicle_verified=0,source_limits='Public-reference catalog; NOT a complete licensed 2026 SAE annex or OEM factory database.'),indent=2),encoding='utf-8')
    print('SIGNAL COUNTS',counts,'TOTAL',len(master),'SUBCHANNELS',len(channels),'DB_INTEGRITY_OK')
if __name__=='__main__':build()
