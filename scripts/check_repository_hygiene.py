from __future__ import annotations

import subprocess
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
MAX_BYTES = 50 * 1024 * 1024
FORBIDDEN = (
    "apps/desktop/src-tauri/music-studio-runtime/",
    "apps/desktop/src-tauri/private-beta-pack/",
    ".local/music-generation/",
    "release-assets/",
)


def candidates() -> list[str]:
    result = subprocess.run(
        ["git", "ls-files", "-co", "--exclude-standard", "-z"],
        cwd=ROOT,
        check=True,
        capture_output=True,
    )
    return [value.decode("utf-8") for value in result.stdout.split(b"\0") if value]


def main() -> int:
    failures: list[str] = []
    for relative in candidates():
        normalized = relative.replace("\\", "/")
        if normalized.startswith(FORBIDDEN):
            failures.append(f"forbidden release payload: {normalized}")
            continue
        path = ROOT / relative
        if path.is_file() and path.stat().st_size > MAX_BYTES:
            failures.append(f"ordinary Git file exceeds 50 MiB: {normalized}")
    if failures:
        raise SystemExit("\n".join(failures))
    print("repository hygiene: ok")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
