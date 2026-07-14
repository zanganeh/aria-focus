[CmdletBinding()]
param(
  [string]$Destination = (Join-Path $PSScriptRoot '..\apps\desktop\src-tauri\review-candidates'),
  [string]$Masters = (Join-Path $PSScriptRoot '..\.local\music-generation\runs\deep-work-still-calibration-v2-run-2\masters'),
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
  @{ File='deep-work-still-cloud-070.flac'; Hash='945c74c1f7aed0ce7858d1de6ab241c7f48e34b0e58fdfd31cdf6d97950a727d'; Bytes=6977340 },
  @{ File='deep-work-still-ember-072.flac'; Hash='2e263cb45dbcbf31647f5a3e955e471b11d6c730e95ef76ff1623d2559e101e9'; Bytes=6556767 },
  @{ File='deep-work-still-dusk-068.flac'; Hash='2c94f4dd431f177257764f3a0b2bd8b7465d9b4856da14cd54417ab0731d2ce1'; Bytes=6786809 },
  @{ File='deep-work-still-tide-074.flac'; Hash='5e82108abccb8853dd92c073c79f439744dc75eea2785081447ec626b92481bc'; Bytes=6916187 }
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
  Write-Host "Cleaned quarantined review staging directory."
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
  Write-Host "Staged all four hash-pinned quarantined review candidates in $destinationFull"
} finally {
  if ($temporary -and (Test-Path -LiteralPath $temporary)) {
    if (-not (Test-LinkOrReparse $temporary)) { Remove-Item -LiteralPath $temporary -Recurse -Force }
  }
}
