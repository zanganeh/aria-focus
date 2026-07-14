# Aria Music Admin

Music Admin is a native, local-only Rust application for operating the existing
Aria Focus production tools. It does not contain a web server and does not
publish anything automatically.

Run it from the repository:

```powershell
pnpm music:admin
```

Build a standalone internal executable:

```powershell
pnpm music:admin:build
```

The executable is written to `target/release/music-admin.exe`. Keep the
repository beside it: generation plans, pinned local models, evidence tools,
and release configuration remain repository-owned inputs.

The app provides:

- single-track plan creation and generation;
- safe updates by cloning a track into a new plan revision;
- batch creation, batch cloning, and batch generation;
- FLAC master-pack and Ogg Opus distribution-pack creation;
- visible background job logs;
- per-track generation progress with completed/total counts;
- in-app FLAC preview with play, pause, resume, stop, and volume;
- full local verification and release-identity checks;
- the reviewed GitHub workflow command without executing publication.

Plan revisions are create-only. Music Admin refuses to replace an existing
plan, and the production pipeline continues to bind each run to the exact plan
hash that generated it.
