#!/usr/bin/env python3
"""Build the signed `runtime/` package consumed by the desktop installer."""
from __future__ import annotations
import argparse, base64, hashlib, json, os, shutil
from pathlib import Path

SOURCE_INCLUDE = (".venv", "ace-step-source", "snapshots", "model-snapshots-manifest.json")
WORKER_NAME = "studio_worker.py"
EXCLUDED = {".git", "runs", "logs", "cache", "__pycache__", ".pytest_cache"}

def safe_path(value: str) -> Path:
    path = Path(value).expanduser().resolve()
    if not path.is_absolute() or path == Path(path.anchor): raise argparse.ArgumentTypeError("an absolute, non-root path is required")
    return path

def version(value: str) -> str:
    if not value or value != value.strip() or any(c not in "abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789._-" for c in value): raise argparse.ArgumentTypeError("version must contain only letters, numbers, '.', '_' or '-'")
    return value

def allowed(relative: Path) -> bool: return not any(part in EXCLUDED or part.endswith(".log") for part in relative.parts)
def reject_link(path: Path) -> None:
    if path.is_symlink(): raise ValueError(f"symlinks/reparse points are not allowed: {path}")
def digest(path: Path) -> tuple[str,int]:
    h=hashlib.sha256(); n=0
    with path.open("rb") as f:
        for b in iter(lambda:f.read(1024*1024),b""): h.update(b); n+=len(b)
    return h.hexdigest(),n
def listed(source: Path):
    files=[]
    for name in SOURCE_INCLUDE:
        item=source/name
        if not item.exists(): raise ValueError(f"required input is missing: {name}")
        reject_link(item)
        candidates=[item] if item.is_file() else sorted(item.rglob("*"))
        for file in candidates:
            reject_link(file)
            if file.is_file() and allowed(file.relative_to(source)):
                sha,n=digest(file); files.append({"path":file.relative_to(source).as_posix(),"sha256":sha,"bytes":n})
    worker = Path(__file__).with_name(WORKER_NAME)
    if not worker.is_file(): raise ValueError(f"packaged worker is missing: {WORKER_NAME}")
    reject_link(worker)
    sha,n=digest(worker); files.append({"path":WORKER_NAME,"sha256":sha,"bytes":n})
    return sorted(files,key=lambda f:f["path"])
def copy_tree(source:Path, destination:Path) -> None:
    for rel in sorted((p.relative_to(source) for p in source.rglob("*")),key=lambda p:p.as_posix()):
        item=source/rel; reject_link(item)
        if not allowed(rel): continue
        target=destination/rel
        if item.is_dir(): target.mkdir(parents=True,exist_ok=True)
        elif item.is_file(): target.parent.mkdir(parents=True,exist_ok=True); shutil.copy2(item,target)
def private_key(value: str):
    try: from cryptography.hazmat.primitives.serialization import load_pem_private_key
    except ImportError as e: raise SystemExit("cryptography is required to sign runtime packages") from e
    return load_pem_private_key(safe_path(value).read_bytes(),password=None)
def main() -> int:
    p=argparse.ArgumentParser();p.add_argument("--source",required=True,type=safe_path);p.add_argument("--output",required=True,type=safe_path);p.add_argument("--version",required=True,type=version);p.add_argument("--private-key",default=os.getenv("MUSIC_STUDIO_RUNTIME_SIGNING_KEY"));p.add_argument("--dry-run",action="store_true");a=p.parse_args()
    if not a.source.is_dir() or a.output.exists() or a.output.is_relative_to(a.source): p.error("source must exist; output must be a new directory outside source")
    if not a.private_key: p.error("--private-key or MUSIC_STUDIO_RUNTIME_SIGNING_KEY is required")
    try: files=listed(a.source)
    except ValueError as e: p.error(str(e))
    manifest={"format":1,"runtime_version":a.version,"required_bytes":sum(x["bytes"] for x in files),"files":files}
    canonical=json.dumps(manifest,sort_keys=True,separators=(",",":")).encode(); signature=private_key(a.private_key).sign(canonical)
    if a.dry_run: print(canonical.decode()); return 0
    a.output.mkdir(); runtime=a.output/"runtime"; runtime.mkdir()
    for name in SOURCE_INCLUDE:
        item=a.source/name
        if item.is_dir(): copy_tree(item,runtime/name)
        else: shutil.copy2(item,runtime/name)
    shutil.copy2(Path(__file__).with_name(WORKER_NAME), runtime/WORKER_NAME)
    (a.output/"package-manifest.json").write_bytes(canonical);(a.output/"package-manifest.sig").write_text(base64.b64encode(signature).decode()+"\n",encoding="ascii")
    return 0
if __name__=="__main__": raise SystemExit(main())
