$ErrorActionPreference = 'Stop'
$env:ARIA_FOCUS_BUNDLED_PACK_DIR = '../../../content/opus-release/private-beta-pack'
pnpm -C apps/desktop tauri build --features bundled-listening-test --config src-tauri/tauri.opus-listening-test.conf.json
if ($LASTEXITCODE -ne 0) {
    exit $LASTEXITCODE
}
