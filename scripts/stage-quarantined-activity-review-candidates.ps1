[CmdletBinding()]
param(
  [string]$Destination = (Join-Path $PSScriptRoot '..\apps\desktop\src-tauri\activity-review-candidates'),
  [switch]$Clean,
  # Test-only fault injection proving temporary debris is removed after a
  # later copy failure; never used by the normal staging command.
  [int]$FailAfterCopyForTest = 0
)

$ErrorActionPreference = 'Stop'
$root = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..'))
$tauriRoot = [IO.Path]::GetFullPath((Join-Path $root 'apps\desktop\src-tauri'))
$creativityMasters = [IO.Path]::GetFullPath((Join-Path $root '.local\music-generation\runs\creativity-multigenre-calibration-v1\masters'))
$motivationMasters = [IO.Path]::GetFullPath((Join-Path $root '.local\music-generation\runs\motivation-multigenre-calibration-v1\masters'))
$lightWorkMasters = [IO.Path]::GetFullPath((Join-Path $root '.local\music-generation\runs\light-work-multigenre-calibration-v1\masters'))
$candidates = @(
  @{ File='creativity-softmotion-downtempo-086.flac'; Root=$creativityMasters; Hash='75fdcc6b23b967fcf82a10f45ea423af884a9da0b5c0529fdfcff40ea2ce63c2'; Bytes=6795987 },
  @{ File='creativity-threadlight-classical-068.flac'; Root=$creativityMasters; Hash='2e41ceaf7c7970385fb66bcc8fdf3f011da42758210f64ac0b5c42726037d9e3'; Bytes=7133308 },
  @{ File='creativity-prismfield-ambient-078.flac'; Root=$creativityMasters; Hash='fd9dc24b17d15407197a1dfdccb8020f7cfcbe35c06ebb16e5078898893d57e0'; Bytes=6677526 },
  @{ File='creativity-inkroom-jazz-074.flac'; Root=$creativityMasters; Hash='36050ea9d3357503949587cf6115c4df84c96564c68dc1ad056a729e9a009687'; Bytes=6986845 },
  @{ File='motivation-groundbeat-hiphop-092.flac'; Root=$motivationMasters; Hash='d0a72851e5bc94272962aaaaee9d9ccace0271caa033dabe594e5b3ede25c765'; Bytes=6703878 },
  @{ File='motivation-riseplain-orchestral-088.flac'; Root=$motivationMasters; Hash='0d529d4190cc10ff85806233a0d3cc00d20b41130e1157574c35e27f5281bcbf'; Bytes=7664752 },
  @{ File='motivation-forwardgrid-electronic-104.flac'; Root=$motivationMasters; Hash='bcae720112549ab9f39bb42c5d4ab976fbc362b9ab78bcfb7f363b1bea8bb5ef'; Bytes=6745563 },
  @{ File='motivation-neonsteady-synthwave-096.flac'; Root=$motivationMasters; Hash='bcf0c7ca1795c16d350a0724340d2ed903c7c40e95881a59ef9273b0378b510b'; Bytes=7078482 },
  @{ File='lightwork-sunpaper-acoustic-082.flac'; Root=$lightWorkMasters; Hash='88252f9b24616503ee56f48b0e10e769f8c48b4f4e4e80132bc65ace7aa7c301'; Bytes=7260313 },
  @{ File='lightwork-glassair-electronic-086.flac'; Root=$lightWorkMasters; Hash='f0a5390a6ad40d1d7b4293695ddddb4ff9fef5533028d4d035f70aa58234b9f0'; Bytes=6816697 },
  @{ File='lightwork-windowtable-jazz-078.flac'; Root=$lightWorkMasters; Hash='6c7d7a3bce1b17c5938652890b3fdd94ab229dcf8cf5dbcdac80a0f7182bb9ed'; Bytes=7772910 },
  @{ File='lightwork-easystep-downtempo-090.flac'; Root=$lightWorkMasters; Hash='9bd98ead130db86a83e370226e2df082d46f81a77a984698acb9236eff9d1749'; Bytes=6857619 }
)

function Test-LinkOrReparse([string]$Path) {
  $item = Get-Item -LiteralPath $Path -Force
  return [bool]($item.Attributes -band [IO.FileAttributes]::ReparsePoint)
}
function Assert-Within([string]$Path, [string]$Parent, [string]$What) {
  $full = [IO.Path]::GetFullPath($Path)
  $parentFull = [IO.Path]::GetFullPath($Parent).TrimEnd('\') + '\'
  if (-not $full.StartsWith($parentFull, [StringComparison]::OrdinalIgnoreCase)) { throw "$What must remain inside $Parent" }
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
  if ((Get-FileHash -LiteralPath $Path -Algorithm SHA256).Hash.ToLowerInvariant() -ne $Candidate.Hash) { throw "${What} hash changed: $($Candidate.File)" }
}
function Assert-DeclaredContents([string]$Path) {
  $actual = @(Get-ChildItem -LiteralPath $Path -Force | ForEach-Object { $_.Name } | Sort-Object)
  $expected = @($candidates | ForEach-Object { $_.File } | Sort-Object)
  if (($actual.Count -ne $expected.Count) -or (Compare-Object $actual $expected)) { throw 'Staging directory must contain exactly the declared candidates.' }
  foreach ($candidate in $candidates) { Assert-Exact (Join-Path $Path $candidate.File) $candidate 'Staged candidate' }
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
  Get-ChildItem -LiteralPath $destinationParent -Directory -Force -Filter '.activity-review-stage-*' | ForEach-Object {
    if (-not (Test-LinkOrReparse $_.FullName)) { Remove-Item -LiteralPath $_.FullName -Recurse -Force }
  }
  Write-Host 'Cleaned Activity Review quarantined staging directory.'
  exit 0
}

if (Test-Path -LiteralPath $destinationFull) { throw "Refusing to clobber existing staging destination: $destinationFull" }
foreach ($masters in @($creativityMasters, $motivationMasters, $lightWorkMasters)) { Assert-PlainDirectory $masters 'Pinned masters directory' }
foreach ($candidate in $candidates) { Assert-Exact (Join-Path $candidate.Root $candidate.File) $candidate 'Pinned master' }

$temporary = Join-Path $destinationParent ('.activity-review-stage-' + [guid]::NewGuid().ToString('N'))
try {
  New-Item -ItemType Directory -Path $temporary | Out-Null
  $copied = 0
  foreach ($candidate in $candidates) {
    Copy-Item -LiteralPath (Join-Path $candidate.Root $candidate.File) -Destination (Join-Path $temporary $candidate.File) -ErrorAction Stop
    $copied++
    if ($FailAfterCopyForTest -gt 0 -and $copied -ge $FailAfterCopyForTest) { throw 'Injected staging copy failure for test.' }
  }
  Assert-DeclaredContents $temporary
  Move-Item -LiteralPath $temporary -Destination $destinationFull -ErrorAction Stop
  $temporary = $null
  Assert-DeclaredContents $destinationFull
  Write-Host "Staged all twelve hash-pinned Activity Review candidates in $destinationFull"
} finally {
  if ($temporary -and (Test-Path -LiteralPath $temporary)) {
    if (-not (Test-LinkOrReparse $temporary)) { Remove-Item -LiteralPath $temporary -Recurse -Force }
  }
}
