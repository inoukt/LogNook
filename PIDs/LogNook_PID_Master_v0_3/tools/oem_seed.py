"""Small, attributable OEM examples. Reference entries are NEVER enabled by brand alone."""
from standard_seed import SOURCE
TOYOTA='https://raw.githubusercontent.com/OBDb/Toyota/main/signalsets/v3/default.json'
FORD='https://raw.githubusercontent.com/OBDb/Ford-Mustang/main/signalsets/v3/default.json'
NISSAN='https://raw.githubusercontent.com/OBDb/Nissan-LEAF/main/signalsets/v3/default.json'
BMW='https://raw.githubusercontent.com/OBDb/BMW-3-Series/main/signalsets/v3/default.json'
HONDA='https://raw.githubusercontent.com/OBDb/Honda-Civic/main/signalsets/v3/default.json'
SUBARU='https://raw.githubusercontent.com/RomRaider/RomRaider/master/definitions/log_defs.xml'
# DID|name|header|response|bits|offset_bits|formula|unit; fields curated from public source.
TOYOTA_DATA='''
0103|ECU odometer|700|708|24|8|raw|km
1002|12V starter battery voltage|700|708|16|0|raw*12.207/10000|V
1017|Fuel injection volume|700|708|16|0|raw/3200|L
1018|Port injector pulse width|700|708|16|0|raw/1000|ms
102A|Learned fuel trim idle bank 1|700|708|8|0|raw*100/255-50|%
102A|Learned fuel trim low bank 1|700|708|8|8|raw*100/255-50|%
102A|Learned fuel trim mid-1 bank 1|700|708|8|16|raw*100/255-50|%
102A|Learned fuel trim mid-2 bank 1|700|708|8|24|raw*100/255-50|%
102A|Learned fuel trim high bank 1|700|708|8|32|raw*100/255-50|%
1041|Coolant temperature at ignition|700|708|8|0|raw*0.625-40|°C
1041|Coolant temperature initial|700|708|8|8|raw*0.625-40|°C
1046|Cooling fan duty|700|708|8|0|raw*0.5|%
105E|Engine ECU internal temperature|700|708|8|0|raw-40|°C
1062|Exhaust VVT OCV duty|700|708|16|0|raw*400/65535|%
106F|Intake VVT target bank 1|700|708|16|0|raw*639.9/65535|deg
1071|Exhaust VVT target bank 1|700|708|16|0|raw*639.9/65535|deg
1087|Intake VVT change bank 1|700|708|16|0|raw*639.9/65535|deg
1088|Exhaust VVT change bank 1|700|708|16|0|raw*639.9/65535|deg
10A5|Misfire count all cylinders|700|708|8|0|raw|count
10A7|Misfire count cylinder 1|700|708|8|0|raw|count
10A7|Misfire count cylinder 2|700|708|8|8|raw|count
10A7|Misfire count cylinder 3|700|708|8|16|raw|count
10A7|Misfire count cylinder 4|700|708|8|24|raw|count
'''
FORD_DATA='''
0301|MAP sensor voltage|7E0||16|0|raw*0.0009765625|V
0307|Low-pressure fuel pump commanded duty|7E0||16|0|raw*0.0030517578|%
030F|Commanded lambda|7E0||16|0|raw*0.0000305176|lambda
'''
# Subaru SSM base memory offsets. offset is NOT a UDS DID or J1979 PID.
SUBARU_DATA='''
000E|Engine speed|16|raw/4|rpm
000D|Manifold absolute pressure|8|raw*37/255|psi
000C|Air fuel learning 2|8|(raw-128)*100/128|%
000B|Air fuel correction 2|8|(raw-128)*100/128|%
000A|Air fuel learning 1|8|(raw-128)*100/128|%
0009|Air fuel correction 1|8|(raw-128)*100/128|%
0008|Engine coolant temperature|8|raw-40|°C
001A|Front oxygen sensor 2 voltage|16|raw*5/1000|V
0018|Rear oxygen sensor voltage|16|raw*5/1000|V
0016|Front oxygen sensor 1 voltage|16|raw*5/1000|V
0015|Throttle opening angle|8|raw*100/255|%
0013|Mass airflow|16|raw/100|g/s
0012|Intake air temperature|8|raw-40|°C
0011|Ignition timing|8|(raw-128)/2|deg
0010|Vehicle speed|8|raw|km/h
0023|Atmospheric pressure|8|raw*37/255|psi
0022|Knock correction|8|(raw-128)/2|deg
0021|Injector pulse width cylinder 2|8|raw*256/1000|ms
0020|Injector pulse width cylinder 1|8|raw*256/1000|ms
001E|Throttle sensor voltage|8|raw*0.02|V
001D|MAF sensor voltage|8|raw*0.02|V
001C|Battery voltage|8|raw*0.08|V
002A|Fuel temperature|8|raw-40|°C
0029|Accelerator opening angle|8|raw*100/256|%
0028|Learned ignition timing|8|(raw-128)/2|deg
0024|Relative manifold pressure|8|(raw-128)*37/255|psi
0032|Canister purge control duty|8|raw*100/255|%
0031|Secondary wastegate duty|8|raw*100/255|%
0030|Primary wastegate duty|8|raw*100/255|%
003F|Left intake oil control duty|8|raw*100/255|%
003E|Right intake oil control duty|8|raw*100/255|%
003D|Left VVT advance|8|raw-50|deg
003C|Right VVT advance|8|raw-50|deg
'''
NISSAN_DATA='''
0E01|Odometer|743|763|24|0|raw|km
0E25|Front right tire pressure|743|763|8|0|raw*1.7236894|kPa
0E26|Front left tire pressure|743|763|8|0|raw*1.7236894|kPa
0E27|Rear right tire pressure|743|763|8|0|raw*1.7236894|kPa
0E28|Rear left tire pressure|743|763|8|0|raw*1.7236894|kPa
0E2E|Remaining electric range|743|763|16|0|raw|km
1103|12V battery voltage|797|79A|8|0|raw/12.5|V
1146|Motor power|797|79A|16|0|raw*40|W
1151|A/C and PTC heater electrical power|797|79A|8|0|raw*250|W
1152|Auxiliary electrical power|797|79A|8|0|raw*100|W
121A|Vehicle speed|797|79A|16|0|raw/10|km/h
1236|High-voltage battery charge power|797|79A|16|0|raw*100|W
'''
BMW_DATA='''
6335|High voltage battery state of health|6F1|607|8|24|raw|%
DD68|High voltage battery voltage|6F1|607|16|0|raw/100|V
DD69|High voltage battery current|6F1|607|32|0|signed32(raw)/100|A
DDBC|High voltage state of charge|6F1|607|16|0|raw/10|%
DDBC|High voltage state of charge max|6F1|607|16|16|raw/10|%
DDBC|High voltage state of charge min|6F1|607|16|32|raw/10|%
'''
HONDA_DATA='''
2028|Hybrid battery individual cell voltage A1|DA01|01|16|216|raw/5|mV
2028|Hybrid battery individual cell voltage A2|DA01|01|16|232|raw/5|mV
'''


VW='https://raw.githubusercontent.com/OBDb/Volkswagen-Golf/main/signalsets/v3/default.json'
CHEVY='https://raw.githubusercontent.com/OBDb/Chevrolet-Bolt-EV/main/signalsets/v3/default.json'
HYUNDAI='https://raw.githubusercontent.com/OBDb/Hyundai-IONIQ-5/main/signalsets/v3/default.json'
MAZDA='https://raw.githubusercontent.com/OBDb/Mazda-CX-5/main/signalsets/v3/default.json'
JEEP='https://raw.githubusercontent.com/OBDb/Jeep-Wrangler/main/signalsets/v3/default.json'
VW_DATA='''
2A07|Starter battery voltage|710|77A|16|0|raw/1000+4|V
2A0B|Battery temperature|710|77A|8|0|raw-40|°C
2A0C|Battery state of charge|710|77A|8|0|raw|%
1800|Front left wheel speed|713|77D|16|0|raw/10|km/h
1801|Front right wheel speed|713|77D|16|0|raw/10|km/h
1802|Rear left wheel speed|713|77D|16|0|raw/10|km/h
1803|Rear right wheel speed|713|77D|16|0|raw/10|km/h
2605|A/C refrigerant pressure|746|7B0|8|0|raw*0.2|bar
'''
CHEVY_DATA='''
000D|Vehicle speed|7E0|7E8|8|0|raw|km/h
1144|A/C high side sensor voltage|7E0|7E8|8|0|raw/50|V
1564|A/C high side pressure|7E0|7E8|8|0|raw|kPa
1C38|14V power module set point|7E0|7E8|8|0|raw*20/51|%
1C47|14V module target voltage|7E0|7E8|8|0|raw/10|V
232C|Secondary ambient air temperature|7E0|7E8|8|0|raw-40|°C
'''
HYUNDAI_DATA='''
E001|Auxiliary battery voltage|744|74C|16|88|raw/100|V
E001|DC charging state|744|74C|1|140|raw|boolean
'''
MAZDA_DATA='''
2A05|Rear left tire pressure|720|728|8|0|raw*1373/100000|bar
2A06|Front left tire pressure|720|728|8|0|raw*1373/100000|bar
2A07|Rear right tire pressure|720|728|8|0|raw*1373/100000|bar
2A08|Front right tire pressure|720|728|8|0|raw*1373/100000|bar
2A0A|Rear left tire temperature|720|728|8|0|raw-50|°C
2A0B|Front left tire temperature|720|728|8|0|raw-50|°C
'''
JEEP_DATA='''
A0A1|Front left tire pressure altitude compensated|740|4C0|8|0|raw|psi
A0A1|Front right tire pressure altitude compensated|740|4C0|8|8|raw|psi
A0A1|Rear right tire pressure altitude compensated|740|4C0|8|16|raw|psi
A0A1|Rear left tire pressure altitude compensated|740|4C0|8|24|raw|psi
A0F1|Front left tire pressure|740|4C0|8|8|raw/4|psi
A0F1|Front left tire temperature|740|4C0|8|16|raw-50|°C
A0F2|Front right tire pressure|740|4C0|8|8|raw/4|psi
A0F2|Front right tire temperature|740|4C0|8|16|raw-50|°C
'''

def make(brand,model,source_id,url,did,name,hdr,rax,bits,bix,formula,unit,index=0,service='22',address=''):
    uid=''.join(c if c.isalnum() else '_' for c in name.upper()).strip('_')
    return dict(signal_id=f'{brand.upper()}_{model.upper().replace("-","_").replace(" ","_")}_{did}_{uid}',layer='SSM_MEMORY' if service=='SSM_READ' else 'OEM_DID',brand=brand,model_filter=model,year_filter='',ecu_filter='ECU_ID_REQUIRED' if brand=='Subaru' else 'EXACT_ECU_ID_REQUIRED',transport='SUBARU_SSM' if brand=='Subaru' else 'ISO_TP_OR_VENDOR_DIAG',service=service,identifier='' if service=='SSM_READ' else did,memory_offset=address,request_header=hdr,response_header=rax,name=name,category='OEM research candidate',bytes=(int(bits)+7)//8,bit_offset=int(bix),bit_length=int(bits),value_format='signed' if 'signed' in formula else 'uint',formula=formula,unit=unit,decoder='candidate_oem',source_id=source_id,source_url=url,research_state='community_reference',vehicle_validation='not_tested',auto_request=False,notes='NOT validated on user test vehicles. Import and confirm source year filter, ECU ID, address and response layout before enabling; some source entries are marked debug.')

def records():
    result=[]
    for row in TOYOTA_DATA.strip().splitlines():
        did,name,hdr,rax,bits,bix,formula,unit=row.split('|')
        result.append(make('Toyota','brand_reference','OBDB_TOYOTA',TOYOTA,did,name,hdr,rax,bits,bix,formula,unit))
    for row in FORD_DATA.strip().splitlines():
        did,name,hdr,rax,bits,bix,formula,unit=row.split('|')
        result.append(make('Ford','Mustang_reference_not_Flex','OBDB_FORD_MUSTANG',FORD,did,name,hdr,rax,bits,bix,formula,unit))
    for row in SUBARU_DATA.strip().splitlines():
        address,name,bits,formula,unit=row.split('|')
        result.append(make('Subaru','SSM_base_compatible_ecus_only','ROMRAIDER_SSM',SUBARU,'',name,'','',bits,'0',formula,unit,service='SSM_READ',address=address))
    for brand,model,src,url,hunk in [('Nissan','LEAF','OBDB_NISSAN_LEAF',NISSAN,NISSAN_DATA),('BMW','3_series_hybrid_variants','OBDB_BMW_3',BMW,BMW_DATA),('Honda','Civic_hybrid_2023plus','OBDB_HONDA_CIVIC',HONDA,HONDA_DATA)]:
        for row in hunk.strip().splitlines():
            did,name,hdr,rax,bits,bix,formula,unit=row.split('|')
            result.append(make(brand,model,src,url,did,name,hdr,rax,bits,bix,formula,unit))
    for brand,model,src,url,hunk in [
        ('Volkswagen','Golf_selected_variants','OBDB_VW_GOLF',VW,VW_DATA),
        ('Chevrolet','Bolt_EV_selected_variants','OBDB_CHEVY_BOLT',CHEVY,CHEVY_DATA),
        ('Hyundai','IONIQ_5_selected_variants','OBDB_HYUNDAI_IONIQ5',HYUNDAI,HYUNDAI_DATA),
        ('Mazda','CX_5_selected_variants','OBDB_MAZDA_CX5',MAZDA,MAZDA_DATA),
        ('Jeep','Wrangler_selected_variants','OBDB_JEEP_WRANGLER',JEEP,JEEP_DATA)]:
        for row in hunk.strip().splitlines():
            did,name,hdr,rax,bits,bix,formula,unit=row.split('|')
            result.append(make(brand,model,src,url,did,name,hdr,rax,bits,bix,formula,unit))
    return result
