from __future__ import annotations
import json, re, sqlite3
from pathlib import Path
from datetime import date

DB = Path(__file__).resolve().parents[1] / 'data' / 'lognook_pids.sqlite'

CONCEPTS = [
('engine_rpm','Engine RPM','Engine','rpm',['rpm','engine speed','crankshaft speed']),
('vehicle_speed','Vehicle speed','Movement','kilometersPerHour',['vehicle speed','vss','road speed']),
('coolant_temp','Engine coolant temperature','Engine','celsius',['coolant','ect','engine coolant']),
('intake_air_temp','Intake air temperature','Engine','celsius',['iat','intake air temperature']),
('oil_temp','Engine oil temperature','Engine','celsius',['oil temperature','engine oil temp']),
('oil_pressure','Engine oil pressure','Engine','kilopascal',['oil pressure','engine oil pressure']),
('map','Manifold absolute pressure','Engine','kilopascal',['map','manifold absolute pressure']),
('baro','Barometric pressure','Engine','kilopascal',['baro','barometric pressure','atmospheric pressure']),
('boost_actual','Boost pressure actual','Engine','kilopascal',['boost actual','boost pressure','turbo pressure']),
('boost_target','Boost pressure target','Engine','kilopascal',['boost target','desired boost','commanded boost']),
('wastegate_cmd','Wastegate command','Engine','percent',['wastegate duty','wastegate command','wgdc']),
('wastegate_pos','Wastegate position','Engine','percent',['wastegate position']),
('throttle_pos','Throttle position','Control','percent',['throttle position','throttle angle']),
('throttle_cmd','Commanded throttle','Control','percent',['commanded throttle','throttle command']),
('accelerator_pos','Accelerator pedal position','Control','percent',['accelerator pedal','pedal position','app']),
('maf','Mass air flow','Engine','gramsPerSecond',['maf','mass air flow']),
('engine_load','Engine load','Engine','percent',['engine load','calculated load','absolute load']),
('spark_advance','Ignition timing / spark advance','Engine','degrees',['spark advance','ignition timing','spark timing']),
('knock_feedback','Knock feedback / retard','Engine','degrees',['knock feedback','knock retard','kr']),
('knock_learned','Learned knock correction','Engine','degrees',['knock learned','learned knock','knock correction learned']),
('lambda_cmd','Commanded lambda / equivalence ratio','Fuel','lambda',['commanded lambda','target lambda','equivalence ratio commanded']),
('lambda_actual','Measured lambda / AFR','Fuel','lambda',['lambda actual','measured lambda','air fuel ratio','a/f sensor']),
('stft_b1','Short-term fuel trim bank 1','Fuel','percent',['stft bank 1','short term fuel trim bank 1']),
('ltft_b1','Long-term fuel trim bank 1','Fuel','percent',['ltft bank 1','long term fuel trim bank 1']),
('stft_b2','Short-term fuel trim bank 2','Fuel','percent',['stft bank 2','short term fuel trim bank 2']),
('ltft_b2','Long-term fuel trim bank 2','Fuel','percent',['ltft bank 2','long term fuel trim bank 2']),
('fuel_pressure_low','Low-side fuel pressure','Fuel','kilopascal',['low fuel pressure','low-side fuel pressure','low pressure fuel']),
('fuel_pressure_high_actual','High-side fuel pressure actual','Fuel','kilopascal',['high fuel pressure actual','rail pressure actual','fuel rail pressure actual']),
('fuel_pressure_high_target','High-side fuel pressure target','Fuel','kilopascal',['high fuel pressure target','rail pressure target','desired fuel rail pressure']),
('fuel_pump_duty','Fuel pump duty','Fuel','percent',['fuel pump duty','low pressure fuel pump duty','lpfp duty']),
('injector_pw','Injector pulse width','Fuel','milliseconds',['injector pulse width','injection time','injector port']),
('fuel_rate','Fuel rate','Fuel','gramsPerSecond',['fuel rate','engine fuel rate']),
('fuel_level','Fuel level','Fuel','percent',['fuel level','tank level']),
('vvt_intake_target','Intake cam/VVT target','Engine','degrees',['intake vvt target','intake cam target','intake cam desired']),
('vvt_intake_actual','Intake cam/VVT actual','Engine','degrees',['intake vvt actual','intake cam actual','intake vvt change']),
('vvt_exhaust_target','Exhaust cam/VVT target','Engine','degrees',['exhaust vvt target','exhaust cam target']),
('vvt_exhaust_actual','Exhaust cam/VVT actual','Engine','degrees',['exhaust vvt actual','exhaust cam actual','exhaust vvt change']),
('misfire_total','Misfire total','Engine.Misfires','count',['misfire count all','misfire total','all cylinders misfire']),
('misfire_cyl1','Misfire cylinder 1','Engine.Misfires','count',['misfire cylinder 1','misfire count cylinder 1']),
('misfire_cyl2','Misfire cylinder 2','Engine.Misfires','count',['misfire cylinder 2','misfire count cylinder 2']),
('misfire_cyl3','Misfire cylinder 3','Engine.Misfires','count',['misfire cylinder 3','misfire count cylinder 3']),
('misfire_cyl4','Misfire cylinder 4','Engine.Misfires','count',['misfire cylinder 4','misfire count cylinder 4']),
('misfire_cyl5','Misfire cylinder 5','Engine.Misfires','count',['misfire cylinder 5','misfire count cylinder 5']),
('misfire_cyl6','Misfire cylinder 6','Engine.Misfires','count',['misfire cylinder 6','misfire count cylinder 6']),
('trans_fluid_temp','Transmission fluid temperature','Transmission','celsius',['transmission fluid temperature','trans fluid temp','atf temp']),
('trans_gear','Transmission gear','Transmission','scalar',['gear','actual gear','commanded gear']),
('trans_input_speed','Transmission input/turbine speed','Transmission','rpm',['input shaft speed','turbine speed','trans input speed']),
('trans_output_speed','Transmission output speed','Transmission','rpm',['output shaft speed','trans output speed']),
('converter_slip','Torque converter slip','Transmission','rpm',['converter slip','torque converter slip']),
('wheel_speed_fl','Front-left wheel speed','Movement','kilometersPerHour',['front left wheel speed','wheel speed fl']),
('wheel_speed_fr','Front-right wheel speed','Movement','kilometersPerHour',['front right wheel speed','wheel speed fr']),
('wheel_speed_rl','Rear-left wheel speed','Movement','kilometersPerHour',['rear left wheel speed','wheel speed rl']),
('wheel_speed_rr','Rear-right wheel speed','Movement','kilometersPerHour',['rear right wheel speed','wheel speed rr']),
('steering_angle','Steering wheel angle','Control','degrees',['steering wheel angle','steering angle']),
('yaw_rate','Yaw rate','Orientation','degreesPerSecond',['yaw rate']),
('lateral_g','Lateral acceleration','Orientation','gravity',['lateral g','lateral acceleration']),
('longitudinal_g','Longitudinal acceleration','Orientation','gravity',['longitudinal g','longitudinal acceleration']),
('brake_pressure','Brake pressure','Brakes','kilopascal',['brake pressure','wheel cylinder pressure']),
('battery_12v','12V battery voltage','Battery','volts',['12v battery voltage','starter battery voltage','battery voltage']),
('battery_soc','Battery state of charge','Battery','percent',['battery state of charge','soc']),
('battery_soh','Battery state of health','Battery','percent',['battery state of health','soh']),
('hv_battery_voltage','HV battery voltage','Battery','volts',['high voltage battery voltage','hv battery voltage']),
('hv_battery_current','HV battery current','Battery','amps',['high voltage battery current','hv battery current']),
('charge_power','Charging power','Battery.Charging','kilowatts',['charge power','charging power']),
('dpf_soot','DPF soot load','Emissions','percent',['dpf soot','soot load']),
('dpf_diff_pressure','DPF differential pressure','Emissions','kilopascal',['dpf differential pressure']),
('egt','Exhaust gas temperature','Emissions','celsius',['exhaust gas temperature','egt']),
('def_level','DEF level','Emissions','percent',['def level','diesel exhaust fluid level']),
('nox','NOx concentration','Emissions','ppm',['nox concentration','nox sensor']),
('ptu_temp','Power transfer unit temperature','Drivetrain','celsius',['power transfer unit temperature','ptu temperature','ptu sump temperature']),
('awd_torque','AWD/4WD requested torque','Drivetrain','newtonMeters',['awd torque','requested torque capacity','power transfer unit requested torque']),
('tpms_fl','Front-left tire pressure','Tires','psi',['front left tire pressure']),
('tpms_fr','Front-right tire pressure','Tires','psi',['front right tire pressure']),
('tpms_rl','Rear-left tire pressure','Tires','psi',['rear left tire pressure']),
('tpms_rr','Rear-right tire pressure','Tires','psi',['rear right tire pressure']),
]

def add_col(con, table, col, decl):
    cols={r[1] for r in con.execute(f'PRAGMA table_info({table})')}
    if col not in cols: con.execute(f'ALTER TABLE {table} ADD COLUMN {col} {decl}')

def migrate(con):
    for col,decl in [
      ('module_type',"TEXT NOT NULL DEFAULT ''"),('suggested_metric',"TEXT NOT NULL DEFAULT ''"),
      ('decoder_json',"TEXT NOT NULL DEFAULT '{}'"),('min_value','REAL'),('max_value','REAL'),
      ('source_scope',"TEXT NOT NULL DEFAULT ''"),('applicability_specificity','INTEGER NOT NULL DEFAULT 0'),
      ('confidence',"TEXT NOT NULL DEFAULT 'research'"),('notes',"TEXT NOT NULL DEFAULT ''")]:
        add_col(con,'oem_candidates',col,decl)
    con.executescript('''
    CREATE TABLE IF NOT EXISTS canonical_concepts(
      concept_id TEXT PRIMARY KEY, name TEXT NOT NULL, category TEXT NOT NULL, preferred_unit TEXT NOT NULL DEFAULT '',
      description TEXT NOT NULL DEFAULT ''
    );
    CREATE TABLE IF NOT EXISTS concept_aliases(
      concept_id TEXT NOT NULL, alias TEXT NOT NULL, PRIMARY KEY(concept_id,alias),
      FOREIGN KEY(concept_id) REFERENCES canonical_concepts(concept_id)
    );
    CREATE TABLE IF NOT EXISTS candidate_concepts(
      candidate_id TEXT NOT NULL, concept_id TEXT NOT NULL, match_method TEXT NOT NULL,
      match_score REAL NOT NULL DEFAULT 0, PRIMARY KEY(candidate_id,concept_id),
      FOREIGN KEY(candidate_id) REFERENCES oem_candidates(candidate_id),
      FOREIGN KEY(concept_id) REFERENCES canonical_concepts(concept_id)
    );
    CREATE TABLE IF NOT EXISTS vehicle_profiles(
      vehicle_id TEXT PRIMARY KEY, brand TEXT NOT NULL, model TEXT NOT NULL, generation TEXT NOT NULL DEFAULT '',
      model_year INTEGER, engine TEXT NOT NULL DEFAULT '', transmission TEXT NOT NULL DEFAULT '', drivetrain TEXT NOT NULL DEFAULT '',
      vin_pattern TEXT NOT NULL DEFAULT '', notes TEXT NOT NULL DEFAULT '', physical_test_available INTEGER NOT NULL DEFAULT 0
    );
    CREATE TABLE IF NOT EXISTS vehicle_supported_commands(
      profile_id TEXT NOT NULL, make TEXT NOT NULL, model TEXT NOT NULL,
      model_year_from INTEGER NOT NULL, model_year_to INTEGER NOT NULL, reported_model_year INTEGER,
      ecu_header TEXT NOT NULL, command_code TEXT NOT NULL, signal_names_json TEXT NOT NULL,
      source_record TEXT NOT NULL, applicability_status TEXT NOT NULL, validation_status TEXT NOT NULL,
      auto_request INTEGER NOT NULL DEFAULT 0 CHECK(auto_request=0), notes TEXT NOT NULL DEFAULT '',
      PRIMARY KEY(profile_id,ecu_header,command_code)
    );
    CREATE TABLE IF NOT EXISTS validation_sessions(
      session_id TEXT PRIMARY KEY, vehicle_id TEXT NOT NULL, started_at TEXT NOT NULL, adapter TEXT NOT NULL DEFAULT '',
      adapter_firmware TEXT NOT NULL DEFAULT '', os TEXT NOT NULL DEFAULT '', app_version TEXT NOT NULL DEFAULT '', notes TEXT NOT NULL DEFAULT '',
      FOREIGN KEY(vehicle_id) REFERENCES vehicle_profiles(vehicle_id)
    );
    CREATE TABLE IF NOT EXISTS validation_results(
      session_id TEXT NOT NULL, candidate_id TEXT NOT NULL,
      result TEXT NOT NULL CHECK(result IN ('SUPPORTED','UNSUPPORTED','NO_RESPONSE','NEGATIVE_RESPONSE','WRONG_LENGTH','BAD_DECODER','PLAUSIBLE','PHYSICALLY_VERIFIED','SKIPPED')),
      response_length INTEGER, raw_response_hex TEXT NOT NULL DEFAULT '', decoded_value TEXT NOT NULL DEFAULT '',
      sample_count INTEGER NOT NULL DEFAULT 0, min_observed REAL, max_observed REAL, notes TEXT NOT NULL DEFAULT '',
      PRIMARY KEY(session_id,candidate_id),
      FOREIGN KEY(session_id) REFERENCES validation_sessions(session_id),
      FOREIGN KEY(candidate_id) REFERENCES oem_candidates(candidate_id)
    );
    CREATE TABLE IF NOT EXISTS source_family_aliases(
      family TEXT NOT NULL, brand TEXT NOT NULL, source_brand TEXT NOT NULL, rationale TEXT NOT NULL DEFAULT '',
      PRIMARY KEY(family,brand,source_brand)
    );
    CREATE INDEX IF NOT EXISTS idx_oem_brand_model ON oem_candidates(brand,model_scope);
    CREATE INDEX IF NOT EXISTS idx_oem_service_did ON oem_candidates(service,identifier);
    CREATE INDEX IF NOT EXISTS idx_oem_source ON oem_candidates(source_id);
    CREATE INDEX IF NOT EXISTS idx_oem_name ON oem_candidates(name);
    CREATE INDEX IF NOT EXISTS idx_validation_candidate ON validation_results(candidate_id,result);
    CREATE INDEX IF NOT EXISTS idx_vehicle_commands ON vehicle_supported_commands(make,model,model_year_from,model_year_to,ecu_header);
    ''')
    for cid,name,cat,unit,aliases in CONCEPTS:
        con.execute('INSERT OR REPLACE INTO canonical_concepts(concept_id,name,category,preferred_unit) VALUES(?,?,?,?)',(cid,name,cat,unit))
        for a in aliases: con.execute('INSERT OR IGNORE INTO concept_aliases(concept_id,alias) VALUES(?,?)',(cid,a.lower()))
    vehicles=[
      ('SCION_XB_2014_2AZFE','Scion','xB','E150 facelift',2014,'2.4L 2AZ-FE','','FWD','',
       'Primary physical validation vehicle. Model corpus has Mode 21/22 candidates; do not assume every default definition applies.',1),
      ('FORD_FLEX_2011_35EB','Ford','Flex','D4 platform / first generation',2011,'3.5L EcoBoost','','AWD/FWD','',
       'Primary Ford EcoBoost physical validation vehicle. Flex upstream model corpus is currently empty; prefer ECU-matched evidence over related-model assumptions.',1),
      ('SUBARU_WORKSHOP_GENERIC','Subaru','Multiple','','', '', '', '', '',
       'Workshop validation profile. Create a specific profile with model/year/engine/ECU ID before promoting proprietary signals.',1),
    ]
    con.executemany('''INSERT OR REPLACE INTO vehicle_profiles(vehicle_id,brand,model,generation,model_year,engine,transmission,drivetrain,vin_pattern,notes,physical_test_available)
                       VALUES(?,?,?,?,?,?,?,?,?,?,?)''',vehicles)
    aliases=[
      ('Ford Motor Company','Lincoln','Ford','Shared family fallback only; model/ECU specificity wins.'),
      ('General Motors','GMC','Chevrolet','Many fallback definitions overlap; retain badge-specific source provenance.'),
      ('General Motors','Buick','Chevrolet','Many fallback definitions overlap; retain badge-specific source provenance.'),
      ('General Motors','Cadillac','Chevrolet','Many fallback definitions overlap; retain badge-specific source provenance.'),
      ('Honda Motor Company','Acura','Honda','Shared family research only; never imply universal applicability.'),
      ('Hyundai Motor Group','Kia','Hyundai','Shared family research only; preserve brand/model addressing.'),
      ('Hyundai Motor Group','Genesis','Hyundai','Shared family research only; preserve brand/model addressing.'),
      ('Nissan Motor Corporation','Infiniti','Nissan','Shared family research only; preserve brand/model addressing.'),
      ('Volkswagen Group','Audi','Volkswagen','Large overlap exists but ECU addressing/platform applicability must be checked.'),
      ('Volkswagen Group','Porsche','Volkswagen','Large overlap exists but ECU addressing/platform applicability must be checked.'),
      ('BMW Group','MINI','BMW','Shared family research only; preserve chassis/ECU applicability.'),
    ]
    con.executemany('INSERT OR REPLACE INTO source_family_aliases VALUES(?,?,?,?)',aliases)
    import_vehicle_supported_commands(con)
    con.execute("UPDATE source_corpora SET services_json=? WHERE source_id='OBDB_SCION_XB_DEFAULT'", ('{\"21\":107,\"22\":106}',))
    con.execute("INSERT OR REPLACE INTO db_meta(key,value) VALUES('schema_version','3')")
    con.execute("INSERT OR REPLACE INTO db_meta(key,value) VALUES('deep_catalog_version','0.3')")
    con.execute("INSERT OR REPLACE INTO db_meta(key,value) VALUES('package_version','0.3')")
    con.execute("INSERT OR REPLACE INTO db_meta(key,value) VALUES('synced_oem_candidates',?)",(str(con.execute('SELECT COUNT(*) FROM oem_candidates').fetchone()[0]),))
    con.execute("INSERT OR REPLACE INTO db_meta(key,value) VALUES('deep_catalog_date',?)",(date.today().isoformat(),))
    con.commit()

def import_vehicle_supported_commands(con):
    path=Path(__file__).resolve().parents[1]/'data'/'vehicle_support'/'scion_xb_2008_2014_supported_commands.json'
    if not path.exists(): return
    record=json.loads(path.read_text(encoding='utf-8'))
    years=record['user_requested_model_year_range']
    sql='''INSERT OR REPLACE INTO vehicle_supported_commands(
      profile_id,make,model,model_year_from,model_year_to,reported_model_year,ecu_header,command_code,
      signal_names_json,source_record,applicability_status,validation_status,auto_request,notes
    ) VALUES(?,?,?,?,?,?,?,?,?,?,?,?,?,?)'''
    rows=[]
    for ecu_header,commands in record['supported_commands_by_ecu'].items():
        for command_code,signal_names in commands.items():
            rows.append((record['vehicle_profile_id'],record['make'],record['model'],years['from'],years['to'],
                record.get('reported_model_year'),ecu_header,command_code,
                json.dumps([name.strip() for name in signal_names.split(',') if name.strip()]),
                record['source_record'],record['applicability_status'],record['validation_status'],0,record['notes']))
    con.executemany(sql,rows)

if __name__=='__main__':
    con=sqlite3.connect(DB)
    migrate(con)
    print('migrated',DB)
    print('concepts',con.execute('select count(*) from canonical_concepts').fetchone()[0])
    print('vehicles',con.execute('select count(*) from vehicle_profiles').fetchone()[0])
