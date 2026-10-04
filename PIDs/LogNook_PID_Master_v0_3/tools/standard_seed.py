"""Public-reference J1979 signal catalog. No assumption of vehicle support."""
SOURCE='WIKI_OBD_PUBLIC'
# hex|label|bytes|unit|formula|category. Empty formula means structured or textual decoder.
TEXT='''
00|Supported PIDs 01-20|4|bitmap||Discovery
01|Monitor status since DTC clear|4|structure||Diagnostics
02|Freeze-frame trigger DTC|2|DTC||Diagnostics
03|Fuel system status|2|enum||Fuel
04|Calculated engine load|1|%|A*100/255|Engine
05|Engine coolant temperature|1|°C|A-40|Temperature
06|Short term fuel trim bank 1|1|%|(A-128)*100/128|Fuel
07|Long term fuel trim bank 1|1|%|(A-128)*100/128|Fuel
08|Short term fuel trim bank 2|1|%|(A-128)*100/128|Fuel
09|Long term fuel trim bank 2|1|%|(A-128)*100/128|Fuel
0A|Fuel pressure|1|kPa|A*3|Fuel
0B|Intake manifold absolute pressure|1|kPa|A|Air
0C|Engine speed|2|rpm|((A*256)+B)/4|Engine
0D|Vehicle speed|1|km/h|A|Movement
0E|Timing advance|1|°BTDC|A/2-64|Ignition
0F|Intake air temperature|1|°C|A-40|Temperature
10|Mass air flow|2|g/s|((A*256)+B)/100|Air
11|Throttle position|1|%|A*100/255|Control
12|Commanded secondary air status|1|enum||Emissions
13|O2 sensors present banks 1-2|1|bitmap||Oxygen
1C|OBD standard supported|1|enum||Identity
1D|O2 sensors present banks 1-4|1|bitmap||Oxygen
1E|Auxiliary input status|1|bitmap||Control
1F|Runtime since engine start|2|s|A*256+B|Engine
20|Supported PIDs 21-40|4|bitmap||Discovery
21|Distance driven with MIL|2|km|A*256+B|Diagnostics
22|Fuel rail pressure relative|2|kPa|(A*256+B)*0.079|Fuel
23|Fuel rail gauge pressure|2|kPa|(A*256+B)*10|Fuel
2C|Commanded EGR|1|%|A*100/255|Emissions
2D|EGR error|1|%|(A-128)*100/128|Emissions
2E|Commanded evaporative purge|1|%|A*100/255|Emissions
2F|Fuel tank level|1|%|A*100/255|Fuel
30|Warm-ups since DTCs cleared|1|count|A|Diagnostics
31|Distance since DTCs cleared|2|km|A*256+B|Diagnostics
32|Evap vapor pressure|2|Pa|signed16(A*256+B)/4|Emissions
33|Absolute barometric pressure|1|kPa|A|Air
3C|Catalyst temp bank 1 sensor 1|2|°C|(A*256+B)/10-40|Emissions
3D|Catalyst temp bank 2 sensor 1|2|°C|(A*256+B)/10-40|Emissions
3E|Catalyst temp bank 1 sensor 2|2|°C|(A*256+B)/10-40|Emissions
3F|Catalyst temp bank 2 sensor 2|2|°C|(A*256+B)/10-40|Emissions
40|Supported PIDs 41-60|4|bitmap||Discovery
41|Drive cycle monitor status|4|structure||Diagnostics
42|Control module voltage|2|V|(A*256+B)/1000|Electrical
43|Absolute engine load|2|%|(A*256+B)*100/255|Engine
44|Commanded equivalence ratio lambda|2|lambda|(A*256+B)*2/65536|Fuel
45|Relative throttle position|1|%|A*100/255|Control
46|Ambient air temperature|1|°C|A-40|Temperature
47|Absolute throttle position B|1|%|A*100/255|Control
48|Absolute throttle position C|1|%|A*100/255|Control
49|Accelerator pedal position D|1|%|A*100/255|Control
4A|Accelerator pedal position E|1|%|A*100/255|Control
4B|Accelerator pedal position F|1|%|A*100/255|Control
4C|Commanded throttle actuator|1|%|A*100/255|Control
4D|Runtime with MIL|2|min|A*256+B|Diagnostics
4E|Time since DTCs cleared|2|min|A*256+B|Diagnostics
4F|Maximum lambda O2 voltage current MAP|4|structure||Oxygen
50|Maximum air flow|4|g/s|A*10|Air
51|Fuel type|1|enum||Fuel
52|Ethanol fuel percentage|1|%|A*100/255|Fuel
53|Absolute evaporative vapor pressure|2|kPa|(A*256+B)/200|Emissions
54|Evaporative vapor pressure signed|2|Pa|signed16(A*256+B)|Emissions
55|STFT secondary O2 banks 1 and 3|2|structure||Fuel
56|LTFT secondary O2 banks 1 and 3|2|structure||Fuel
57|STFT secondary O2 banks 2 and 4|2|structure||Fuel
58|LTFT secondary O2 banks 2 and 4|2|structure||Fuel
59|Fuel rail absolute pressure|2|kPa|(A*256+B)*10|Fuel
5A|Relative accelerator pedal|1|%|A*100/255|Control
5B|Hybrid battery remaining life|1|%|A*100/255|Hybrid EV
5C|Engine oil temperature|1|°C|A-40|Temperature
5D|Fuel injection timing|2|deg|(A*256+B)/128-210|Fuel
5E|Engine fuel rate|2|L/h|(A*256+B)/20|Fuel
5F|Vehicle emission requirements|1|bitmap||Emissions
60|Supported PIDs 61-80|4|bitmap||Discovery
61|Driver demand engine torque|1|%|A-125|Engine
62|Actual engine torque|1|%|A-125|Engine
63|Engine reference torque|2|Nm|A*256+B|Engine
64|Engine torque points|5|structure||Engine
65|Auxiliary input output supported|2|bitmap||Control
66|Multiple mass airflow sensors|5|structure||Air
67|Multiple coolant temperature sensors|3|structure||Temperature
68|Multiple intake air temperature sensors|3|structure||Temperature
69|EGR actual commanded and error|7|structure||Emissions
6A|Diesel intake air flow control|5|structure||Diesel
6B|EGR temperature|5|structure||Emissions
6C|Throttle actuator actual command|5|structure||Control
6D|Fuel pressure control system|11|structure||Fuel
6E|Injection pressure control system|9|structure||Fuel
6F|Turbo compressor inlet pressure|3|structure||Turbo
70|Boost pressure control|10|structure||Turbo
71|Variable geometry turbo control|6|structure||Turbo
72|Wastegate control|5|structure||Turbo
73|Exhaust pressure|5|structure||Exhaust
74|Turbocharger RPM|5|structure||Turbo
75|Turbocharger temperature sensor 1|7|structure||Turbo
76|Turbocharger temperature sensor 2|7|structure||Turbo
77|Charge air cooler temperature|5|structure||Turbo
78|EGT bank 1 sensors|9|structure||Exhaust
79|EGT bank 2 sensors|9|structure||Exhaust
7A|Diesel DPF differential pressure|7|structure||Diesel
7B|Diesel particulate filter|7|structure||Diesel
7C|DPF temperature|9|structure||Diesel
7D|NOx NTE control status|1|bitmap||Diesel
7E|PM NTE control status|1|bitmap||Diesel
7F|Engine runtime detail|13|structure||Engine
80|Supported PIDs 81-A0|4|bitmap||Discovery
81|AECD runtime first group|41|structure||Emissions
82|AECD runtime second group|41|structure||Emissions
83|NOx sensor|9|structure||Diesel
84|Manifold surface temperature|1|°C||Temperature
85|NOx reagent system|10|structure||Diesel
86|Particulate matter sensor|5|structure||Diesel
87|Intake manifold absolute pressure multiple|5|structure||Air
88|SCR inducement system|13|structure||Diesel
89|AECD runtime group 11-15|41|structure||Emissions
8A|AECD runtime group 16-20|41|structure||Emissions
8B|Diesel aftertreatment|7|structure||Diesel
8C|Wide-range O2 sensor data|17|structure||Oxygen
8D|Throttle position G|1|%|A*100/255|Control
8E|Engine friction torque percentage|1|%|A-125|Engine
8F|PM sensor banks 1-2|7|structure||Diesel
90|WWH OBD vehicle information 1|3|structure||Diagnostics
91|WWH OBD vehicle information 2|5|structure||Diagnostics
92|Fuel system control|2|structure||Fuel
93|WWH OBD counters support|3|structure||Diagnostics
94|NOx warning and inducement system|12|structure||Diesel
98|Exhaust gas temperature sensor A|9|structure||Exhaust
99|Exhaust gas temperature sensor B|9|structure||Exhaust
9A|Hybrid EV battery voltage|6|structure||Hybrid EV
9B|Diesel exhaust fluid sensor|4|structure||Diesel
9C|Oxygen sensor detailed data|17|structure||Oxygen
9D|Engine fuel rate grams|4|structure||Fuel
9E|Engine exhaust flow rate|2|structure||Exhaust
9F|Fuel system percentage use|9|structure||Fuel
A0|Supported PIDs A1-C0|4|bitmap||Discovery
A1|NOx sensor corrected data|9|structure||Diesel
A2|Cylinder fuel rate|2|mg/stroke|(A*256+B)/32|Fuel
A3|Evaporative vapor pressure advanced|9|structure||Emissions
A4|Transmission actual gear ratio|4|structure||Transmission
A5|Commanded DEF dosing|4|structure||Diesel
A6|Odometer|4|km|(A*16777216+B*65536+C*256+D)/10|Movement
A7|NOx sensor 3 and 4 concentration|4|structure||Diesel
A8|NOx corrected concentration 3 and 4|4|structure||Diesel
A9|ABS disable switch state|4|structure||Chassis
C0|Supported PIDs C1-E0|4|bitmap||Discovery
C3|Fuel level inputs A and B|2|structure||Fuel
C4|Exhaust particulate diagnostic time count|8|structure||Diesel
C5|Fuel pressure A and B|4|structure||Fuel
C6|Particulate-control diagnostic status|7|structure||Diesel
C7|Distance since module replacement or reflash|2|km|A*256+B|Diagnostics
C8|NOx and particulate warning lamp status|1|bitmap||Diesel
'''

def records():
    data=[]
    for line in TEXT.strip().splitlines():
        pid,name,raw,unit,formula,category=line.split('|')
        state='review_required' if raw=='structure' or unit=='structure' or (not formula and unit not in ('bitmap','enum','DTC')) or int(pid,16)>=0xC3 or pid=='02' else 'public_reference'
        data.append(dict(signal_id='SAE_01_'+pid,layer='SAE_MODE01',brand='ALL',model_filter='',year_filter='',ecu_filter='',transport='OBD_GENERIC',service='01',identifier=pid,memory_offset='',request_header='',response_header='',name=name,category=category,bytes=int(raw),bit_offset='',bit_length='',value_format='uint',formula=formula,unit=unit,decoder='numeric' if formula else ('bitfield' if unit=='bitmap' else 'enum' if unit=='enum' else 'structured_pending'),source_id=SOURCE,source_url='https://en.wikipedia.org/wiki/OBD-II_PIDs',research_state=state,vehicle_validation='not_tested',auto_request=state=='public_reference' and (formula!='' or unit in ('bitmap','enum')),notes=('Service 01 PID 02 is inconsistently listed in public tables; use Service 02 PID 02 for freeze frame DTC and do not auto-poll 0102.' if pid=='02' else 'Query 01 support bitmap first; no guaranteed vehicle support.')))
    for i in range(8):
        p=f'{0x14+i:02X}'
        data.append(dict(signal_id='SAE_01_'+p,layer='SAE_MODE01',brand='ALL',model_filter='',year_filter='',ecu_filter='',transport='OBD_GENERIC',service='01',identifier=p,memory_offset='',request_header='',response_header='',name=f'Narrowband O2 sensor {i+1} voltage and STFT',category='Oxygen',bytes=2,bit_offset='',bit_length='',value_format='pair',formula='V=A/200; STFT=(B-128)*100/128 if B!=255',unit='V;%',decoder='structured_pending',source_id=SOURCE,source_url='https://en.wikipedia.org/wiki/OBD-II_PIDs',research_state='review_required',vehicle_validation='not_tested',auto_request=False,notes='Two sub-channels. B==255 marks STFT unavailable.'))
    for i in range(8):
        for base, kind, comp in [(0x24,'equivalence and voltage','lambda=(A*256+B)*2/65536;voltage=(C*256+D)*8/65536'),(0x34,'equivalence and current','lambda=(A*256+B)*2/65536;current=(C*256+D)/256-128')]:
            p=f'{base+i:02X}'
            data.append(dict(signal_id='SAE_01_'+p,layer='SAE_MODE01',brand='ALL',model_filter='',year_filter='',ecu_filter='',transport='OBD_GENERIC',service='01',identifier=p,memory_offset='',request_header='',response_header='',name=f'Wideband O2 sensor {i+1}: {kind}',category='Oxygen',bytes=4,bit_offset='',bit_length='',value_format='pair',formula=comp,unit='lambda;V' if base==0x24 else 'lambda;mA',decoder='structured_pending',source_id=SOURCE,source_url='https://en.wikipedia.org/wiki/OBD-II_PIDs',research_state='review_required',vehicle_validation='not_tested',auto_request=False,notes='Split into individually defined sub-channels during decoder implementation.'))
    return sorted(data,key=lambda r:int(r['identifier'],16))

SERVICE09={
'00':('Supported service 09 IDs',4,'bitmap'), '01':('VIN message count (legacy transport)',1,'count'), '02':('Vehicle identification number',0,'string'),
'03':('Calibration ID message count',1,'count'),'04':('Calibration identification',0,'string'),'05':('CVN message count',1,'count'),
'06':('Calibration verification numbers',0,'hex'),'07':('In-use performance tracking count',1,'count'),
'08':('In-use performance tracking spark',0,'structure'),'09':('ECU name message count',1,'count'),
'0A':('ECU name',0,'string'),'0B':('In-use performance tracking diesel',0,'structure')}
