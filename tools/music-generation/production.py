#!/usr/bin/env python3
"""Strict offline production wrapper for ACE-Step 1.5 pinned source."""
from __future__ import annotations
import argparse, datetime as dt, hashlib, json, os, platform, re, shutil, stat, subprocess, sys
from pathlib import Path

COMMIT = '6d467e4b5081ccb0abf1ec1bf4fdf9051a2d34b0'
TURBO_REV = '19671f406d603126926c1b7e2adc169acbcade22'
PLANNER_REV = '148d8ea0225bdab342ee1ae3a354275ccd60ca80'
EVIDENCE_SHA256 = '3cc581f1c62f1f0a816234bb8e41a309cb7cc2ff0c83624c290cf1c1532e67a1'
PARAMS = {'batch_size','cfg_interval_end','cfg_interval_start','guidance_scale','keyscale','lm_cfg_scale','lm_temperature','lm_top_k','lm_top_p','thinking','timesignature','use_adg'}
ALLOWED_DURATIONS = {90.0, 180.0}
TURBO_REPO = 'ACE-Step/Ace-Step1.5'
TURBO_ALLOW_PATTERNS = ['.gitattributes', 'README.md', 'config.json', 'Qwen3-Embedding-0.6B/**', 'acestep-v15-turbo/**', 'vae/**']
TURBO_ALLOWED_ROOTS = {'.gitattributes', 'README.md', 'config.json', 'Qwen3-Embedding-0.6B', 'acestep-v15-turbo', 'vae'}
RUN_ID = re.compile(r'^[A-Za-z0-9][A-Za-z0-9._-]{0,127}$')
WINDOWS_DEVICE_NAMES = {'CON','PRN','AUX','NUL',*(f'COM{i}' for i in range(1,10)),*(f'LPT{i}' for i in range(1,10))}
IDENTITY_FILE = 'plan-identity.json'

class PlanContext:
    def __init__(self, path: Path, identity: str, digest: str, plan: dict):
        self.path, self.identity, self.sha256, self.plan = path, identity, digest, plan

def sha256(path: Path) -> str:
    h=hashlib.sha256()
    with path.open('rb') as f:
        for b in iter(lambda:f.read(1024*1024),b''): h.update(b)
    return h.hexdigest()
def utc() -> str: return dt.datetime.now(dt.timezone.utc).replace(microsecond=0).isoformat().replace('+00:00','Z')
def is_reparse(path: Path) -> bool:
    return bool(getattr(path.stat(), 'st_file_attributes', 0) & 0x400)
def load_plan(root: Path, selected: Path) -> PlanContext:
    """Read one regular, non-link plan into an immutable operation snapshot."""
    path=selected if selected.is_absolute() else root/selected
    if path.is_symlink() or is_reparse(path): raise RuntimeError('plan must not be a symlink or reparse point')
    canonical=path.resolve(strict=True)
    if canonical.is_symlink() or is_reparse(canonical): raise RuntimeError('plan must not be a symlink or reparse point')
    flags=os.O_RDONLY | getattr(os, 'O_BINARY', 0) | getattr(os, 'O_NOFOLLOW', 0)
    try: fd=os.open(path, flags)
    except OSError as exc: raise RuntimeError(f'cannot safely open plan: {path}') from exc
    try:
        before=os.fstat(fd)
        if not stat.S_ISREG(before.st_mode) or getattr(before, 'st_file_attributes', 0) & 0x400: raise RuntimeError('plan must be a regular file')
        with os.fdopen(fd, 'rb', closefd=False) as f: data=f.read()
        after=os.fstat(fd)
        if (before.st_dev,before.st_ino,before.st_size,before.st_mtime_ns)!=(after.st_dev,after.st_ino,after.st_size,after.st_mtime_ns): raise RuntimeError('plan changed while being read')
    finally: os.close(fd)
    try: plan=json.loads(data.decode('utf-8'))
    except (UnicodeDecodeError,json.JSONDecodeError) as exc: raise RuntimeError('plan is not valid UTF-8 JSON') from exc
    try: identity=canonical.relative_to(root.resolve()).as_posix()
    except ValueError: identity=str(canonical)
    return PlanContext(canonical,identity,hashlib.sha256(data).hexdigest(),plan)
def cmd(args, cwd=None, stdout=None, env=None): return subprocess.run(args,cwd=cwd,stdout=stdout,stderr=subprocess.STDOUT,text=True,check=False,env=env)
def validate(root: Path, selected: Path) -> PlanContext:
    context=load_plan(root,selected); plan=context.plan
    if sha256(root/'content/evidence/ace-step-1.5-terms-2026-07-11.md') != EVIDENCE_SHA256: raise RuntimeError('terms evidence SHA-256 mismatch')
    pin=plan['batch']['generator_pin']
    expected={'source_commit':COMMIT,'turbo_vae_revision':TURBO_REV,'planner_revision':PLANNER_REV,'python_version':'3.12','config':'acestep-v15-turbo','planner':'acestep-5Hz-lm-0.6B'}
    if any(pin.get(k)!=v for k,v in expected.items()): raise RuntimeError('plan generator pins do not match production contract')
    for c in plan['candidates']:
        if c['duration_seconds'] not in ALLOWED_DURATIONS or c['bpm']<=0 or c['inference']['codec']!='flac' or c['inference']['sample_rate_hz']!=48000 or c['inference']['steps']!=8 or c['inference']['solver']!='ode' or c['inference']['use_random_seed'] is not False: raise RuntimeError(f"{c['id']}: fixed production values mismatch")
        got={p['name'] for p in c['inference']['parameters']}
        if got != PARAMS: raise RuntimeError(f"{c['id']}: unknown or unmapped parameters: {sorted(got ^ PARAMS)}")
    r=cmd(['cargo','run','-q','-p','candidate-ledger','--bin','candidate-ledger','--','validate-plan','--plan',str(context.path)],root)
    if r.returncode: raise RuntimeError('candidate-ledger rejected plan')
    return context
def source(root): return root/'.local/music-generation/ace-step-source'
def local(root): return root/'.local/music-generation'
def snapshot_manifest(root: Path):
    out=[]
    for d in (local(root)/'snapshots').glob('*'):
        if d.is_dir():
            for p in sorted(d.rglob('*')):
                if p.is_file(): out.append({'snapshot':d.name,'path':p.relative_to(d).as_posix(),'bytes':p.stat().st_size,'sha256':sha256(p)})
    (local(root)/'model-snapshots-manifest.json').write_text(json.dumps({'generated_at':utc(),'files':out},indent=2)+'\n')
def turbo_payload_paths(dest: Path):
    return [p.relative_to(dest).as_posix() for p in dest.rglob('*') if p.is_file() and '.cache' not in p.relative_to(dest).parts and p.name != 'snapshot-revision.json']
def validate_turbo_tree(dest: Path):
    bad=[p for p in turbo_payload_paths(dest) if p.split('/',1)[0] not in TURBO_ALLOWED_ROOTS]
    if bad: raise RuntimeError(f'Turbo/VAE snapshot has forbidden payload paths: {bad}')
    roots={p.split('/',1)[0] for p in turbo_payload_paths(dest)}
    missing=TURBO_ALLOWED_ROOTS-roots
    if missing: raise RuntimeError(f'Turbo/VAE snapshot missing required payload groups: {sorted(missing)}')
def validate_local_snapshots(root: Path):
    turbo=local(root)/'snapshots/turbo-vae'; planner=local(root)/'snapshots/planner-0.6b'
    expected=[(turbo,{'repo_id':TURBO_REPO,'revision':TURBO_REV}),(planner,{'repo_id':'ACE-Step/acestep-5Hz-lm-0.6B','revision':PLANNER_REV})]
    for directory, marker in expected:
        p=directory/'snapshot-revision.json'
        if not p.exists() or json.loads(p.read_text()) != marker: raise RuntimeError(f'invalid immutable snapshot marker: {directory}')
    validate_turbo_tree(turbo)
    required=[turbo/'config.json',turbo/'acestep-v15-turbo/config.json',turbo/'acestep-v15-turbo/model.safetensors',turbo/'Qwen3-Embedding-0.6B/config.json',turbo/'Qwen3-Embedding-0.6B/model.safetensors',turbo/'vae/config.json',turbo/'vae/diffusion_pytorch_model.safetensors',planner/'config.json',planner/'model.safetensors']
    missing=[str(p) for p in required if not p.is_file() or p.stat().st_size == 0]
    if missing: raise RuntimeError(f'missing required local model files: {missing}')
    return turbo, planner
def generation_env(root: Path):
    turbo, _ = validate_local_snapshots(root)
    cache=local(root)/'hf-runtime-cache'; modules=cache/'modules'; modules.mkdir(parents=True,exist_ok=True)
    env=os.environ.copy(); env.update({'ACESTEP_CHECKPOINTS_DIR':str(turbo),'HF_HUB_OFFLINE':'1','TRANSFORMERS_OFFLINE':'1','HF_HOME':str(cache),'HF_MODULES_CACHE':str(modules),'PYTHONUTF8':'1','PYTHONIOENCODING':'utf-8'})
    return env
def generator_python(root: Path) -> Path:
    return local(root)/'.venv/Scripts/python.exe' if os.name == 'nt' else local(root)/'.venv/bin/python'
def generation_command(root: Path, config_path: Path): return [str(generator_python(root)),'cli.py','--backend','pt','--config',str(config_path)]
def verify_pinned_source(root: Path):
    src=source(root)
    if not src.exists(): raise RuntimeError('pinned ACE-Step source is unavailable')
    git=['git','-c',f'safe.directory={src.as_posix()}','-C',str(src)]
    head=subprocess.check_output(git+['rev-parse','HEAD'],text=True).strip()
    clean=subprocess.check_output(git+['status','--porcelain=v1'],text=True).strip()
    if head != COMMIT or clean: raise RuntimeError('ACE-Step source pin or clean-tree verification failed')
def safe_remove_unneeded_turbo_17b(dest: Path):
    base=dest.resolve()
    for rel in ('acestep-5Hz-lm-1.7B', '.cache/huggingface/download/acestep-5Hz-lm-1.7B'):
        target=(dest/rel).resolve()
        if base not in target.parents: raise RuntimeError(f'refusing unsafe cleanup path: {target}')
        if target.exists(): shutil.rmtree(target)
    # Incomplete Xet blobs have no directory association except their metadata;
    # only remove cache entries whose metadata names identify the unneeded group.
    cache=dest/'.cache/huggingface/download'
    if cache.exists():
        for p in cache.rglob('*'):
            if 'acestep-5Hz-lm-1.7B' in p.as_posix() and p.exists():
                resolved=p.resolve()
                if cache.resolve() not in resolved.parents: raise RuntimeError(f'refusing unsafe cache cleanup: {resolved}')
                if p.is_dir(): shutil.rmtree(p)
                else: p.unlink()
def download(root: Path, plan: Path, **_):
    validate(root,plan)
    from huggingface_hub import snapshot_download
    snaps=local(root)/'snapshots'; snaps.mkdir(parents=True,exist_ok=True)
    for repo, rev, name in [(TURBO_REPO,TURBO_REV,'turbo-vae'),('ACE-Step/acestep-5Hz-lm-0.6B',PLANNER_REV,'planner-0.6b')]:
        dest=snaps/name
        marker=dest/'snapshot-revision.json'
        if marker.exists():
            if json.loads(marker.read_text()) != {'repo_id':repo,'revision':rev}: raise RuntimeError(f'{dest} has a different immutable revision marker')
            if name == 'turbo-vae': validate_turbo_tree(dest)
            continue
        # A prior interrupted exact-revision transfer is resumed; completed
        # snapshots are identified with a revision marker, never a cache alias.
        kwargs={'repo_id':repo,'revision':rev,'local_dir':str(dest)}
        if name == 'turbo-vae':
            safe_remove_unneeded_turbo_17b(dest)
            kwargs['allow_patterns']=TURBO_ALLOW_PATTERNS
        snapshot_download(**kwargs)
        if name == 'turbo-vae': validate_turbo_tree(dest)
        marker.write_text(json.dumps({'repo_id':repo,'revision':rev})+'\n')
    snapshot_manifest(root)
def config(c: dict, root: Path, run: Path) -> dict:
    values={p['name']:p['value'] for p in c['inference']['parameters']}
    def typed(v):
        if v in {'true','false'}: return v == 'true'
        try: return int(v) if str(int(float(v))) == v else float(v)
        except ValueError: return v
    # This exactly maps all ledger fields to cli.py GenerationParams/GenerationConfig TOML keys.
    return {'project_root':str(source(root)),'checkpoint_dir':str(local(root)/'snapshots/turbo-vae'),'lm_model_path':str(local(root)/'snapshots/planner-0.6b'),'backend':'pt','device':'cuda','save_dir':str(run/'masters'),'audio_format':'flac','caption':c['prompts']['positive'],'lyrics':'[Instrumental]','instrumental':True,'duration':c['duration_seconds'],'bpm':c['bpm'],'inference_steps':c['inference']['steps'],'seed':c['seed'],'shift':c['inference']['shift'],'infer_method':c['inference']['solver'],'use_random_seed':False,'lm_negative_prompt':c['prompts']['negative'], **{k:typed(v) for k,v in values.items()}}
def toml(v):
    def x(a): return ('true' if a else 'false') if isinstance(a,bool) else str(a) if isinstance(a,(int,float)) else json.dumps(a)
    return ''.join(f'{k} = {x(a)}\n' for k,a in v.items())
def evidence(c, asset, analysis, gpu): return {'schema':'adhd-music.candidate-ledger.generation-evidence','schema_version':1,'candidate_id':c['id'],'generated_at':utc(),'machine':platform.node() or 'unknown-machine','gpu':gpu,'output':{'file_name':asset.name,'bytes':asset.stat().st_size,'codec':'flac','sample_rate_hz':48000,'channels':2,'sha256':sha256(asset)},'analyzer':{'file_name':analysis.name,'sha256':sha256(analysis)},'evidence_file_name':f'{c["id"]}.json','edit_lineage':[]}
def candidate_paths(base: Path, candidate_id: str):
    return (base/'masters'/f'{candidate_id}.flac', base/'analyzer-reports'/f'{candidate_id}.json',
            base/'generation-evidence'/f'{candidate_id}.json', base/'generated-records'/f'{candidate_id}.json',
            base/'logs'/f'{candidate_id}.log', base/'configs'/f'{candidate_id}.toml')
def analyzer_command(asset: Path, report: Path):
    return ['cargo','run','-q','-p','audio-analyzer','--','--input',str(asset),'--output',str(report)]
def ledger_command(root: Path, context: PlanContext, c: dict, asset: Path, report: Path, ev: Path, rec: Path):
    return ['cargo','run','-q','-p','candidate-ledger','--bin','candidate-ledger','--','register-generated','--plan',str(context.path),'--candidate',c['id'],'--asset',str(asset),'--analysis',str(report),'--evidence',str(ev),'--output',str(rec)]
def append_log(log: Path, record: dict):
    with log.open('a',encoding='utf-8') as f: f.write(json.dumps(record)+'\n')
def finalize_candidate(root: Path, context: PlanContext, c: dict, asset: Path, report: Path, ev: Path, rec: Path, log: Path, gpu: str):
    """Produce the immutable post-generation artifacts for one existing master."""
    if not asset.is_file() or asset.stat().st_size == 0: raise RuntimeError(f'{c["id"]}: required candidate master is missing')
    if any(p.exists() for p in (report, ev, rec)): raise RuntimeError(f'{c["id"]}: collision; no paths will be overwritten')
    a=cmd(analyzer_command(asset,report),root)
    append_log(log,{'analyzer_exit_code':a.returncode,'analyzed_at':utc()})
    if a.returncode: raise RuntimeError(f'{c["id"]}: audio-analyzer failed with exit code {a.returncode}')
    # x mode preserves the contract even if another process creates evidence
    # between the collision check and this write.
    try:
        with ev.open('x',encoding='utf-8') as f: f.write(json.dumps(evidence(c,asset,report,gpu),indent=2)+'\n')
    except FileExistsError as exc: raise RuntimeError(f'{c["id"]}: collision; no paths will be overwritten') from exc
    r=cmd(ledger_command(root,context,c,asset,report,ev,rec),root)
    append_log(log,{'ledger_exit_code':r.returncode,'finished_at':utc()})
    if r.returncode: raise RuntimeError(f'{c["id"]}: candidate-ledger failed with exit code {r.returncode}')
def reclaim_incomplete_attempt(c, root: Path, base: Path, asset: Path, report: Path, ev: Path, rec: Path, cfg: Path, log: Path):
    """Remove only the two files of a positively identified interrupted run.

    A result, analysis, evidence, or ledger record is always authoritative: even
    an otherwise matching stopped invocation must be preserved in that case.
    """
    if any(p.exists() for p in (asset, report, ev, rec)) or not (cfg.is_file() and log.is_file()): return False
    expected_config=toml(config(c, root, base))
    if cfg.read_text(encoding='utf-8',errors='replace') != expected_config: return False
    text=log.read_text(encoding='utf-8',errors='replace')
    # Any persisted terminal record wins over recovery.  Check the raw text as
    # the JSON trailer can have been only partially flushed when a process died.
    if any(marker in text for marker in ('"ended_at"', '"exit_code"', '"finished_at"', '"ledger_exit_code"', '"completed_at"')): return False
    command=generation_command(root,cfg)
    offline=('offline mode is enabled' in text and str(local(root)/'snapshots/turbo-vae') in text)
    # Normally the first line is our exact invocation record.  The known killed
    # process never flushed that line; its local-checkpoint/offline tokenizer
    # trail is the only accepted fallback, and the exact TOML above fixes the
    # candidate, run directory, and PT backend.
    header=False
    for line in text.splitlines():
        try: record=json.loads(line)
        except json.JSONDecodeError: continue
        if record.get('command') == command and record.get('config_sha256') == sha256(cfg):
            env=record.get('environment',{})
            header=(env.get('ACESTEP_CHECKPOINTS_DIR') == str(local(root)/'snapshots/turbo-vae') and env.get('HF_HUB_OFFLINE') == '1' and env.get('TRANSFORMERS_OFFLINE') == '1')
            break
    legacy=offline and 'loading 5Hz LM tokenizer' in text
    if not (header or legacy): return False
    cfg.unlink(); log.unlink()
    return True
def validate_run_id(value: str) -> str:
    if not isinstance(value,str) or not RUN_ID.fullmatch(value) or value.rstrip(' .') != value or value.split('.',1)[0].upper() in WINDOWS_DEVICE_NAMES:
        raise RuntimeError('run id must be one safe stable identifier')
    return value
def identity_payload(context: PlanContext, run_id: str) -> dict:
    return {'schema':'adhd-music.run-plan-identity','schema_version':1,'batch_id':context.plan['batch']['id'],'run_id':run_id,'plan_sha256':context.sha256,'plan_path':context.identity}
def verify_run_identity(base: Path, context: PlanContext, run_id: str, *, create=False, allow_legacy=False):
    expected=identity_payload(context,run_id); marker=base/IDENTITY_FILE
    if marker.exists():
        try: actual=json.loads(marker.read_text(encoding='utf-8'))
        except json.JSONDecodeError as exc: raise RuntimeError('invalid run plan identity') from exc
        if actual != expected: raise RuntimeError('run plan identity does not match selected plan or run id')
        return
    legacy=(allow_legacy and context.identity=='content/plans/deep-work-calibration-v1.json' and run_id.startswith('deep-work-calibration-v1-retry-') and (base/'generated-records').is_dir())
    if legacy: return
    if not create: raise RuntimeError('run plan identity is missing')
    try:
        with marker.open('x',encoding='utf-8') as f: f.write(json.dumps(expected,indent=2)+'\n')
    except FileExistsError:
        verify_run_identity(base,context,run_id)
def run(root: Path, plan: Path, run_id=None, **_):
    context=validate(root,plan); validate_local_snapshots(root); verify_pinned_source(root); src=source(root)
    if not generator_python(root).is_file(): raise RuntimeError(f'pinned generator Python is unavailable: {generator_python(root)}')
    batch=validate_run_id(run_id or context.plan['batch']['id']); base=local(root)/'runs'/batch
    for d in ('masters','analyzer-reports','generation-evidence','generated-records','logs','configs'): (base/d).mkdir(parents=True,exist_ok=True)
    verify_run_identity(base,context,batch,create=True)
    gpu=subprocess.check_output(['nvidia-smi','--query-gpu=name','--format=csv,noheader'],text=True).strip()
    failures=[]
    for c in context.plan['candidates']:
        asset,report,ev,rec,log,cfg=candidate_paths(base,c['id'])
        reclaim_incomplete_attempt(c,root,base,asset,report,ev,rec,cfg,log)
        if any(p.exists() for p in (asset,report,ev,rec,cfg)): raise RuntimeError(f'{c["id"]}: collision; no paths will be overwritten')
        cfg.write_text(toml(config(c,root,base)),encoding='utf-8')
        with log.open('x',encoding='utf-8') as f:
            command=generation_command(root,cfg); f.write(json.dumps({'started_at':utc(),'plan_sha256':context.sha256,'plan_path':context.identity,'command':command,'environment':{k:generation_env(root)[k] for k in ('ACESTEP_CHECKPOINTS_DIR','HF_HUB_OFFLINE','TRANSFORMERS_OFFLINE','HF_HOME','HF_MODULES_CACHE')},'config_sha256':sha256(cfg)})+'\n')
            r=cmd(command,src,f,env=generation_env(root)); f.write(json.dumps({'ended_at':utc(),'exit_code':r.returncode})+'\n')
        produced=sorted((base/'masters').glob('*.flac'))
        if r.returncode or not produced:
            failures.append(f'{c["id"]} (exit {r.returncode}, outputs {len(produced)})')
            continue
        newest=max(produced,key=lambda p:p.stat().st_mtime)
        if newest != asset: newest.rename(asset)
        finalize_candidate(root,context,c,asset,report,ev,rec,log,gpu)
    if failures: raise RuntimeError('generation failed: '+', '.join(failures))
def process(root: Path, plan: Path, run_id=None, **_):
    """Finalize a completed local batch.  This path never invokes ACE-Step."""
    if not run_id: raise RuntimeError('process requires --run-id')
    context=validate(root,plan); validate_local_snapshots(root); verify_pinned_source(root)
    run_id=validate_run_id(run_id); base=local(root)/'runs'/run_id
    if not base.is_dir(): raise RuntimeError(f'run does not exist: {run_id}')
    verify_run_identity(base,context,run_id,allow_legacy=True)
    paths=[]
    for c in context.plan['candidates']:
        asset,report,ev,rec,log,_=candidate_paths(base,c['id'])
        if not asset.is_file() or asset.stat().st_size == 0: raise RuntimeError(f'{c["id"]}: required candidate master is missing')
        if any(p.exists() for p in (report,ev,rec)): raise RuntimeError(f'{c["id"]}: collision; no paths will be overwritten')
        if not log.exists(): raise RuntimeError(f'{c["id"]}: candidate log is missing')
        paths.append((c,asset,report,ev,rec,log))
    gpu=subprocess.check_output(['nvidia-smi','--query-gpu=name','--format=csv,noheader'],text=True).strip()
    for c,asset,report,ev,rec,log in paths: finalize_candidate(root,context,c,asset,report,ev,rec,log,gpu)
def preflight(root:Path, plan: Path, **_): validate(root,plan); validate_local_snapshots(root); print('preflight ok')
if __name__=='__main__':
 p=argparse.ArgumentParser(); s=p.add_subparsers(dest='action',required=True)
 for a in ('preflight','download','run','process'):
  q=s.add_parser(a); q.add_argument('--root',type=Path,required=True); q.add_argument('--plan',type=Path,required=True)
 s.choices['run'].add_argument('--run-id')
 s.choices['process'].add_argument('--run-id',required=True)
 q=s.choices['preflight']; q.add_argument('--gpu'); q.add_argument('--free-bytes')
 a=p.parse_args(); kwargs=vars(a).copy(); root=kwargs.pop('root').resolve(); kwargs.pop('action'); {'preflight':preflight,'download':download,'run':run,'process':process}[a.action](root,**kwargs)
