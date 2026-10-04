@echo off
setlocal
cd /d "%~dp0"
python tools\migrate_v0_3.py || exit /b 1
python tools\deep_sync.py --db data\lognook_pids.sqlite --canada-core --cache-dir cache\obdb --ack-license || exit /b 1
python tools\assign_concepts.py --db data\lognook_pids.sqlite --replace || exit /b 1
python tools\export_deep_manifest.py || exit /b 1
python tests\test_v0_2.py || exit /b 1
python tests\test_v0_3.py || exit /b 1
echo LogNook deep PID database build complete.
