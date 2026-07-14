$ErrorActionPreference = 'Stop'
$root = (Resolve-Path (Join-Path $PSScriptRoot '..\..')).Path
$python = Join-Path $root '.local\music-generation\.venv\Scripts\python.exe'
if (-not (Test-Path $python)) { throw 'Run bootstrap.ps1 first.' }
& $python (Join-Path $PSScriptRoot 'production.py') run --root $root --plan (Join-Path $root 'content\plans\deep-work-calibration-v1.json')
exit $LASTEXITCODE
