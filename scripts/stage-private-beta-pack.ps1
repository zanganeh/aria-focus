param(
  [Parameter(Mandatory = $true)][string]$PackDirectory
)

$ErrorActionPreference = 'Stop'
$expectedMasterSha256 = '7bd9a8589a70f531beb87c8b817e69820754b47f153c8fb7ccdfb27183f787fa'
$expectedAssetPath = 'assets/deep-work-still-cloud-070-private-beta-v1.flac'
$source = (Resolve-Path -LiteralPath $PackDirectory).Path
$destination = [System.IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..\apps\desktop\src-tauri\private-beta-pack'))
$destinationParent = Split-Path -Parent $destination
$staging = Join-Path $destinationParent ('.private-beta-pack.stage.' + [Guid]::NewGuid().ToString('N'))
$manifest = Join-Path $source 'manifest.json'

function Assert-PlainFile([string]$Path, [string]$Label) {
  $item = Get-Item -LiteralPath $Path -Force -ErrorAction Stop
  if (-not $item.PSIsContainer -and -not ($item.Attributes -band [IO.FileAttributes]::ReparsePoint)) { return }
  throw "$Label must be a plain file and not a link or reparse point."
}

try {
  if ((Get-Item -LiteralPath $source -Force).Attributes -band [IO.FileAttributes]::ReparsePoint) {
    throw 'PackDirectory must not be a link or reparse point.'
  }
  Assert-PlainFile $manifest 'manifest.json'
  $raw = Get-Content -LiteralPath $manifest -Raw | ConvertFrom-Json
  if ($raw.pack.id -ne 'deep-work-still-calibration-v2' -or $raw.items.Count -ne 1) {
    throw 'Only the selected Track E pack with one item may be staged.'
  }
  if ($raw.items[0].id -ne 'deep-work-still-cloud-070-private-beta-v1' -or $raw.items[0].variants.Count -ne 1) {
    throw 'Private-beta identity must match the finalized Track E item and its single variant.'
  }
  $asset = $raw.items[0].variants[0].asset
  if ($asset.path -ne $expectedAssetPath -or $asset.sha256 -ne $expectedMasterSha256) {
    throw 'Manifest asset identity or hash does not match the finalized Track E master.'
  }
  $assetPath = [System.IO.Path]::GetFullPath((Join-Path $source $asset.path))
  $sourcePrefix = $source.TrimEnd('\') + '\'
  if (-not $assetPath.StartsWith($sourcePrefix, [StringComparison]::OrdinalIgnoreCase)) {
    throw 'Manifest asset path escapes PackDirectory.'
  }
  Assert-PlainFile $assetPath 'private-beta audio asset'
  if ((Get-FileHash -LiteralPath $assetPath -Algorithm SHA256).Hash.ToLowerInvariant() -ne $expectedMasterSha256) {
    throw 'Audio bytes do not match the finalized Track E master.'
  }
  if (Test-Path -LiteralPath $destination) {
    throw 'Refusing to overwrite an existing staged private-beta resource; remove it after verifying its contents.'
  }
  New-Item -ItemType Directory -Path (Join-Path $staging 'assets') -Force | Out-Null
  Copy-Item -LiteralPath $manifest -Destination (Join-Path $staging 'manifest.json')
  Copy-Item -LiteralPath $assetPath -Destination (Join-Path $staging $expectedAssetPath)
  if ((Get-FileHash -LiteralPath (Join-Path $staging $expectedAssetPath) -Algorithm SHA256).Hash.ToLowerInvariant() -ne $expectedMasterSha256) {
    throw 'Staged audio verification failed.'
  }
  Move-Item -LiteralPath $staging -Destination $destination
  Write-Host 'Staged private-beta resource. Build with: pnpm --dir apps/desktop tauri build --config src-tauri/tauri.private-beta.conf.json'
} finally {
  if (Test-Path -LiteralPath $staging) {
    Remove-Item -LiteralPath $staging -Recurse -Force
  }
}
