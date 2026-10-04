$ErrorActionPreference = 'Stop'
$Root = Split-Path -Parent $MyInvocation.MyCommand.Path
Set-Location $Root
python tools/migrate_v0_3.py
python tools/deep_sync.py --db data/lognook_pids.sqlite --canada-core --cache-dir cache/obdb --ack-license
python tools/assign_concepts.py --db data/lognook_pids.sqlite --replace
python tools/export_deep_manifest.py
python tests/test_v0_2.py
python tests/test_v0_3.py
Write-Host "LogNook deep PID database build complete."
