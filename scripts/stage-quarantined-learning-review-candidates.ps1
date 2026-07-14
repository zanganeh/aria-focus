[CmdletBinding()]
param(
  [string]$Destination = (Join-Path $PSScriptRoot '..\apps\desktop\src-tauri\learning-review-candidates'),
  [string]$Masters = (Join-Path $PSScriptRoot '..\.local\music-generation\runs\learning-multigenre-calibration-v1\masters'),
  [switch]$Clean,
  # Test-only fault injection proving the temporary directory is removed after
  # a later copy failure; never used by the normal staging command.
  [int]$FailAfterCopyForTest = 0
)

$ErrorActionPreference = 'Stop'
$root = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..'))
$tauriRoot = [IO.Path]::GetFullPath((Join-Path $root 'apps\desktop\src-tauri'))
$masters = [IO.Path]::GetFullPath($Masters)
$candidates = @(
  @{ File='learning-clearfield-ambient-068.flac'; Hash='96b05e7f1d29c0c23a4c48d3cd28cf43044deaaad5314c98907546ff08b5fa25'; Bytes=6760623 },
  @{ File='learning-mossair-organic-072.flac'; Hash='acaca32844e6e33ce78e32178ea235ccc2b389f59ee3b2ce04b94f42b4b09048'; Bytes=7187104 },
  @{ File='learning-paperlight-classical-064.flac'; Hash='4aadaf220e007f8d28e503f25a10fbb90817586a273f962ef02736fdbbe17ed9'; Bytes=6955029 },
  @{ File='learning-softgrain-lofi-076.flac'; Hash='afe7fbd0ad6d853e619d3b16a5207f4e59ea345d5ea88aec2a8ba9f1ddd8763d'; Bytes=7346575 }
)

function Test-LinkOrReparse([string]$Path) {
  $item = Get-Item -LiteralPath $Path -Force
  return [bool]($item.Attributes -band [IO.FileAttributes]::ReparsePoint)
}
function Assert-Within([string]$Path, [string]$Parent, [string]$What) {
  $full = [IO.Path]::GetFullPath($Path)
  $parentFull = [IO.Path]::GetFullPath($Parent).TrimEnd('\') + '\'
  if (-not $full.StartsWith($parentFull, [StringComparison]::OrdinalIgnoreCase)) {
    throw "$What must remain inside $Parent"
  }
  return $full
}
function Assert-PlainDirectory([string]$Path, [string]$What) {
  if (-not (Test-Path -LiteralPath $Path -PathType Container)) { throw "Missing ${What}: $Path" }
  if (Test-LinkOrReparse $Path) { throw "${What} link/reparse point rejected: $Path" }
}
function Assert-Exact([string]$Path, $Candidate, [string]$What) {
  if (-not (Test-Path -LiteralPath $Path -PathType Leaf)) { throw "Missing ${What}: $Path" }
  if (Test-LinkOrReparse $Path) { throw "${What} link/reparse point rejected: $Path" }
  $item = Get-Item -LiteralPath $Path -Force
  if ($item.Length -ne $Candidate.Bytes) { throw "${What} size changed: $($Candidate.File)" }
  if ((Get-FileHash -LiteralPath $Path -Algorithm SHA256).Hash.ToLowerInvariant() -ne $Candidate.Hash) {
    throw "${What} hash changed: $($Candidate.File)"
  }
}

$destinationFull = Assert-Within $Destination $tauriRoot 'Destination'
$destinationParent = Split-Path -Parent $destinationFull
Assert-PlainDirectory $tauriRoot 'Tauri source root'
Assert-PlainDirectory $destinationParent 'Destination parent'

if ($Clean) {
  if (Test-Path -LiteralPath $destinationFull) {
    if (Test-LinkOrReparse $destinationFull) { throw "Destination link/reparse point rejected: $destinationFull" }
    Remove-Item -LiteralPath $destinationFull -Recurse -Force
  }
  Get-ChildItem -LiteralPath $destinationParent -Directory -Force -Filter '.stage-*' | ForEach-Object {
    if (-not (Test-LinkOrReparse $_.FullName)) { Remove-Item -LiteralPath $_.FullName -Recurse -Force }
  }
  Write-Host "Cleaned Learning quarantined review staging directory."
  exit 0
}

if (Test-Path -LiteralPath $destinationFull) { throw "Refusing to clobber existing staging destination: $destinationFull" }
Assert-PlainDirectory $masters 'Pinned masters directory'
foreach ($candidate in $candidates) { Assert-Exact (Join-Path $masters $candidate.File) $candidate 'Pinned master' }

$temporary = Join-Path $destinationParent ('.stage-' + [guid]::NewGuid().ToString('N'))
try {
  New-Item -ItemType Directory -Path $temporary | Out-Null
  $copied = 0
  foreach ($candidate in $candidates) {
    Copy-Item -LiteralPath (Join-Path $masters $candidate.File) -Destination (Join-Path $temporary $candidate.File) -ErrorAction Stop
    $copied++
    if ($FailAfterCopyForTest -gt 0 -and $copied -ge $FailAfterCopyForTest) { throw 'Injected staging copy failure for test.' }
  }
  foreach ($candidate in $candidates) { Assert-Exact (Join-Path $temporary $candidate.File) $candidate 'Staged candidate' }
  Move-Item -LiteralPath $temporary -Destination $destinationFull -ErrorAction Stop
  $temporary = $null
  Write-Host "Staged all four hash-pinned Learning quarantined review candidates in $destinationFull"
} finally {
  if ($temporary -and (Test-Path -LiteralPath $temporary)) {
    if (-not (Test-LinkOrReparse $temporary)) { Remove-Item -LiteralPath $temporary -Recurse -Force }
  }
}
