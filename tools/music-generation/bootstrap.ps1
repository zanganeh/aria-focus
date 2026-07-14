[CmdletBinding()]
param([switch]$DownloadModels)
$ErrorActionPreference = 'Stop'
$root = (Resolve-Path (Join-Path $PSScriptRoot '..\..')).Path
$local = Join-Path $root '.local\music-generation'
$source = Join-Path $local 'ace-step-source'
$venv = Join-Path $local '.venv'
$plan = Join-Path $root 'content\plans\deep-work-calibration-v1.json'
$evidence = Join-Path $root 'content\evidence\ace-step-1.5-terms-2026-07-11.md'
$commit = '6d467e4b5081ccb0abf1ec1bf4fdf9051a2d34b0'

New-Item -ItemType Directory -Force $local | Out-Null
$log = Join-Path $local 'bootstrap.log'
"$(Get-Date -AsUTC -Format o) bootstrap starting" | Set-Content -NoNewline $log
function Require([string]$name) { if (-not (Get-Command $name -ErrorAction SilentlyContinue)) { throw "Required command not found: $name" } }
Require uv; Require git; Require git-lfs; Require nvidia-smi
$gpu = & nvidia-smi --query-gpu=name,driver_version,memory.total --format=csv,noheader 2>&1
if ($LASTEXITCODE -ne 0) { throw "nvidia-smi failed: $gpu" }
$free = (Get-PSDrive -Name D).Free
if ($free -lt 20GB) { throw 'At least 20 GB free on D: is required.' }
& cargo run -q -p candidate-ledger -- validate-plan --plan $plan
if ($LASTEXITCODE -ne 0) { throw 'candidate-ledger validation failed.' }
$expectedEvidence = '3cc581f1c62f1f0a816234bb8e41a309cb7cc2ff0c83624c290cf1c1532e67a1'
$actualEvidence = (Get-FileHash $evidence -Algorithm SHA256).Hash.ToLower()
if ($actualEvidence -ne $expectedEvidence) { throw "Evidence hash mismatch: $actualEvidence" }
& uv python install 3.12
if ($LASTEXITCODE -ne 0) { throw 'uv Python 3.12 installation failed.' }
if (-not (Test-Path $source)) { & git clone --no-checkout https://github.com/ace-step/ACE-Step-1.5.git $source }
& git -C $source fetch --depth=1 origin $commit
& git -C $source checkout --detach $commit
if ((& git -C $source rev-parse HEAD).Trim() -ne $commit) { throw 'ACE-Step HEAD pin mismatch.' }
if ((& git -C $source status --porcelain).Length -ne 0) { throw 'ACE-Step source tree is not clean.' }
& git -C $source lfs install --local
& git -C $source submodule update --init --recursive
if (-not (Test-Path $venv)) { & uv venv --python 3.12 $venv }
Push-Location $source
$env:VIRTUAL_ENV = $venv
& uv sync --locked --active --no-dev
Pop-Location
if ($LASTEXITCODE -ne 0) { throw 'Pinned ACE-Step dependency installation failed.' }
& $venv\Scripts\python.exe (Join-Path $PSScriptRoot 'production.py') preflight --root $root --gpu "$gpu" --free-bytes $free
if ($LASTEXITCODE -ne 0) { throw 'Python preflight failed.' }
if ($DownloadModels) { & $venv\Scripts\python.exe (Join-Path $PSScriptRoot 'production.py') download --root $root }
"`n$(Get-Date -AsUTC -Format o) bootstrap complete; GPU=$gpu; free_bytes=$free" | Add-Content $log
