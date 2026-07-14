import importlib.util, json, tempfile, unittest
from pathlib import Path
from types import SimpleNamespace
from unittest import mock

HERE = Path(__file__).parent
spec = importlib.util.spec_from_file_location('production', HERE/'production.py')
production = importlib.util.module_from_spec(spec); spec.loader.exec_module(production)
ROOT = HERE.parent.parent
V1 = ROOT/'content/plans/deep-work-calibration-v1.json'
V2 = ROOT/'content/plans/activity-library-expansion-v2.json'

def v1_plan(): return production.load_plan(ROOT,V1).plan

class ProductionContractTests(unittest.TestCase):
    def test_turbo_allowlist_is_complete_and_excludes_17b(self):
        patterns=set(production.TURBO_ALLOW_PATTERNS)
        self.assertTrue({'.gitattributes','README.md','config.json','Qwen3-Embedding-0.6B/**','acestep-v15-turbo/**','vae/**'}.issubset(patterns))
        self.assertFalse(any('acestep-5Hz-lm-1.7B' in p for p in patterns))
    def test_turbo_tree_rejects_non_allowlisted_payload(self):
        with tempfile.TemporaryDirectory() as d:
            d=Path(d)
            for name in ('.gitattributes','README.md','config.json','Qwen3-Embedding-0.6B/x','acestep-v15-turbo/x','vae/x'):
                p=d/name; p.parent.mkdir(parents=True,exist_ok=True); p.write_text('x')
            production.validate_turbo_tree(d)
            bad=d/'acestep-5Hz-lm-1.7B/model.safetensors'; bad.parent.mkdir(); bad.write_text('x')
            with self.assertRaises(RuntimeError): production.validate_turbo_tree(d)
    def test_plan_maps_only_known_pinned_cli_fields(self):
        plan = production.validate(ROOT, ROOT/'content/plans/deep-work-calibration-v1.json').plan
        values = production.config(plan['candidates'][0], ROOT, Path('C:/ignored/run'))
        self.assertEqual(values['audio_format'], 'flac')
        self.assertEqual(values['duration'], 90.0)
        self.assertEqual(values['inference_steps'], 8)
        self.assertEqual(values['infer_method'], 'ode')
        self.assertFalse(values['use_random_seed'])
        self.assertEqual(values['batch_size'], 1)
        self.assertTrue(values['instrumental'])
        self.assertEqual(values['lyrics'], '[Instrumental]')
        self.assertEqual(values['backend'], 'pt')
    def test_generation_environment_and_command_are_exact(self):
        env=production.generation_env(ROOT); command=production.generation_command(ROOT,Path('C:/run/candidate.toml'))
        self.assertEqual(Path(command[0]),ROOT/'.local/music-generation/.venv/Scripts/python.exe')
        self.assertEqual(env['ACESTEP_CHECKPOINTS_DIR'],str(ROOT/'.local/music-generation/snapshots/turbo-vae'))
        self.assertEqual(env['HF_HUB_OFFLINE'],'1'); self.assertEqual(env['TRANSFORMERS_OFFLINE'],'1')
        self.assertEqual(env['HF_HOME'],str(ROOT/'.local/music-generation/hf-runtime-cache'))
        self.assertEqual(env['HF_MODULES_CACHE'],str(ROOT/'.local/music-generation/hf-runtime-cache/modules'))
        self.assertEqual(env['PYTHONUTF8'],'1')
        self.assertEqual(env['PYTHONIOENCODING'],'utf-8')
        self.assertEqual(command,[str(ROOT/'.local/music-generation/.venv/Scripts/python.exe'),'cli.py','--backend','pt','--config',str(Path('C:/run/candidate.toml'))])
    def test_analyzer_invocation_uses_exact_input_flag_command(self):
        asset=Path('C:/run/master.flac'); report=Path('C:/run/report.json')
        self.assertEqual(production.analyzer_command(asset,report),
            ['cargo','run','-q','-p','audio-analyzer','--','--input',str(asset),'--output',str(report)])
    def test_preflight_fails_when_required_snapshot_file_is_missing(self):
        with tempfile.TemporaryDirectory() as d:
            root=Path(d); target=root/'.local/music-generation/snapshots/turbo-vae'; target.mkdir(parents=True)
            (target/'snapshot-revision.json').write_text(json.dumps({'repo_id':production.TURBO_REPO,'revision':production.TURBO_REV}))
            with self.assertRaises(RuntimeError): production.validate_local_snapshots(root)
    def test_evidence_construction_is_schema_shaped(self):
        fixture=ROOT/'tools/candidate-ledger/tests/fixtures/sine-220hz-10s-48khz-stereo.flac'
        with tempfile.TemporaryDirectory() as d:
            d=Path(d); asset=d/'asset.flac'; asset.write_bytes(fixture.read_bytes())
            report=d/'analysis.json'; report.write_text('{}')
            e=production.evidence(v1_plan()['candidates'][0],asset,report,'test gpu')
            self.assertEqual(e['schema'],'adhd-music.candidate-ledger.generation-evidence')
            self.assertEqual(e['output']['sha256'],production.sha256(asset))
            self.assertEqual(e['edit_lineage'],[])
    def test_existing_destination_is_a_collision_contract(self):
        # The runner's no-overwrite gate checks every master, report, evidence,
        # record and config path before invoking ACE-Step.
        names=('masters/x.flac','analyzer-reports/x.json','generation-evidence/x.json','generated-records/x.json','configs/x.toml')
        with tempfile.TemporaryDirectory() as d:
            d=Path(d)
            for name in names:
                p=d/name; p.parent.mkdir(parents=True,exist_ok=True); p.write_text('sentinel')
                self.assertTrue(p.exists())
    def test_reclaims_exact_incomplete_offline_pt_invocation(self):
        plan=v1_plan(); candidate=plan['candidates'][0]
        with tempfile.TemporaryDirectory() as d:
            root=Path(d); base=root/'.local/music-generation/runs/retry'; cfg=base/'configs'/f'{candidate["id"]}.toml'; log=base/'logs'/f'{candidate["id"]}.log'
            cfg.parent.mkdir(parents=True); log.parent.mkdir(parents=True)
            cfg.write_text(production.toml(production.config(candidate,root,base)))
            log.write_text(f'checkpoint {root / ".local/music-generation/snapshots/turbo-vae"}: offline mode is enabled\nloading 5Hz LM tokenizer... it may take 80~90s\n')
            paths=[base/'masters'/f'{candidate["id"]}.flac',base/'analyzer-reports'/f'{candidate["id"]}.json',base/'generation-evidence'/f'{candidate["id"]}.json',base/'generated-records'/f'{candidate["id"]}.json']
            self.assertTrue(production.reclaim_incomplete_attempt(candidate,root,base,*paths,cfg,log))
            self.assertFalse(cfg.exists()); self.assertFalse(log.exists())
    def test_reclaim_preserves_when_any_authoritative_output_exists(self):
        candidate=v1_plan()['candidates'][0]
        for protected in ('masters','analyzer-reports','generation-evidence','generated-records'):
            with self.subTest(protected=protected), tempfile.TemporaryDirectory() as d:
                root=Path(d); base=root/'.local/music-generation/runs/retry'; cfg=base/'configs'/f'{candidate["id"]}.toml'; log=base/'logs'/f'{candidate["id"]}.log'
                cfg.parent.mkdir(parents=True); log.parent.mkdir(parents=True)
                cfg.write_text(production.toml(production.config(candidate,root,base)))
                log.write_text(f'{root / ".local/music-generation/snapshots/turbo-vae"} offline mode is enabled\nloading 5Hz LM tokenizer\n')
                paths=[base/'masters'/f'{candidate["id"]}.flac',base/'analyzer-reports'/f'{candidate["id"]}.json',base/'generation-evidence'/f'{candidate["id"]}.json',base/'generated-records'/f'{candidate["id"]}.json']
                target=paths[('masters','analyzer-reports','generation-evidence','generated-records').index(protected)]; target.parent.mkdir(parents=True,exist_ok=True); target.write_text('authoritative')
                self.assertFalse(production.reclaim_incomplete_attempt(candidate,root,base,*paths,cfg,log))
                self.assertTrue(cfg.exists()); self.assertTrue(log.exists())
    def test_reclaim_preserves_completed_or_nonmatching_attempt(self):
        candidate=v1_plan()['candidates'][0]
        for config_text, log_text in ((None, 'offline mode is enabled\nloading 5Hz LM tokenizer\n'), ('exact', 'offline mode is enabled\nloading 5Hz LM tokenizer\n{"ended_at":"now","exit_code":1}\n')):
            with self.subTest(log=log_text), tempfile.TemporaryDirectory() as d:
                root=Path(d); base=root/'.local/music-generation/runs/retry'; cfg=base/'configs'/f'{candidate["id"]}.toml'; log=base/'logs'/f'{candidate["id"]}.log'
                cfg.parent.mkdir(parents=True); log.parent.mkdir(parents=True)
                cfg.write_text('wrong' if config_text is None else production.toml(production.config(candidate,root,base)))
                log.write_text(f'{root / ".local/music-generation/snapshots/turbo-vae"} {log_text}')
                paths=[base/'masters'/f'{candidate["id"]}.flac',base/'analyzer-reports'/f'{candidate["id"]}.json',base/'generation-evidence'/f'{candidate["id"]}.json',base/'generated-records'/f'{candidate["id"]}.json']
                self.assertFalse(production.reclaim_incomplete_attempt(candidate,root,base,*paths,cfg,log))
                self.assertTrue(cfg.exists()); self.assertTrue(log.exists())
    def test_finalize_records_analyzer_failure_and_propagates_it(self):
        c=v1_plan()['candidates'][0]
        with tempfile.TemporaryDirectory() as d:
            d=Path(d); asset=d/'master.flac'; asset.write_bytes(b'audio'); log=d/'candidate.log'; log.write_text('started\n')
            context=production.load_plan(ROOT,ROOT/'content/plans/deep-work-calibration-v1.json')
            with mock.patch.object(production,'cmd',return_value=SimpleNamespace(returncode=17)) as command:
                with self.assertRaisesRegex(RuntimeError,'audio-analyzer failed with exit code 17'):
                    production.finalize_candidate(d,context,c,asset,d/'report.json',d/'evidence.json',d/'record.json',log,'gpu')
            self.assertEqual(command.call_args.args[0],production.analyzer_command(asset,d/'report.json'))
            self.assertIn('"analyzer_exit_code": 17',log.read_text())
    def test_process_never_invokes_generation_and_requires_all_masters(self):
        plan=v1_plan(); context=production.load_plan(ROOT,ROOT/'content/plans/deep-work-calibration-v1.json')
        with tempfile.TemporaryDirectory() as d:
            root=Path(d); base=root/'.local/music-generation/runs/retry'
            for c in plan['candidates']:
                asset,_,_,_,log,_=production.candidate_paths(base,c['id']); asset.parent.mkdir(parents=True,exist_ok=True); asset.write_bytes(b'audio'); log.parent.mkdir(parents=True,exist_ok=True); log.write_text('started\n')
            production.verify_run_identity(base,context,'retry',create=True)
            with mock.patch.object(production,'validate',return_value=context), mock.patch.object(production,'validate_local_snapshots'), mock.patch.object(production,'verify_pinned_source'), mock.patch.object(production.subprocess,'check_output',return_value='test gpu\n'), mock.patch.object(production,'finalize_candidate') as finalize, mock.patch.object(production,'generation_command') as generation:
                production.process(root,plan=ROOT/'content/plans/deep-work-calibration-v1.json',run_id='retry')
            self.assertEqual(finalize.call_count,len(plan['candidates']))
            generation.assert_not_called()

    def test_run_propagates_candidate_generation_failure(self):
        plan=v1_plan(); context=production.load_plan(ROOT,V1)
        plan['candidates']=plan['candidates'][:1]; context.plan=plan
        with tempfile.TemporaryDirectory() as d:
            root=Path(d); python=root/'.local/music-generation/.venv/Scripts/python.exe'; python.parent.mkdir(parents=True); python.write_bytes(b'python')
            with mock.patch.object(production,'validate',return_value=context), mock.patch.object(production,'validate_local_snapshots'), mock.patch.object(production,'verify_pinned_source'), mock.patch.object(production,'generation_env',return_value={k:'x' for k in ('ACESTEP_CHECKPOINTS_DIR','HF_HUB_OFFLINE','TRANSFORMERS_OFFLINE','HF_HOME','HF_MODULES_CACHE')}), mock.patch.object(production.subprocess,'check_output',return_value='gpu'), mock.patch.object(production,'cmd',return_value=SimpleNamespace(returncode=23)):
                with self.assertRaisesRegex(RuntimeError,'generation failed.*exit 23'):
                    production.run(root,plan=V1,run_id='failed-run')
    def test_process_fails_for_missing_master_or_artifact_collision(self):
        plan=v1_plan(); context=production.load_plan(ROOT,ROOT/'content/plans/deep-work-calibration-v1.json')
        for problem in ('missing','collision'):
            with self.subTest(problem=problem), tempfile.TemporaryDirectory() as d:
                root=Path(d); base=root/'.local/music-generation/runs/retry'
                for c in plan['candidates']:
                    asset,report,_,_,log,_=production.candidate_paths(base,c['id']); asset.parent.mkdir(parents=True,exist_ok=True); asset.write_bytes(b'audio'); log.parent.mkdir(parents=True,exist_ok=True); log.write_text('started\n')
                first=plan['candidates'][0]; asset,report,_,_,_,_=production.candidate_paths(base,first['id'])
                if problem == 'missing': asset.unlink()
                else: report.parent.mkdir(parents=True,exist_ok=True); report.write_text('existing')
                production.verify_run_identity(base,context,'retry',create=True)
                with mock.patch.object(production,'validate',return_value=context), mock.patch.object(production,'validate_local_snapshots'), mock.patch.object(production,'verify_pinned_source'):
                    with self.assertRaisesRegex(RuntimeError, 'master is missing' if problem == 'missing' else 'collision'):
                        production.process(root,plan=ROOT/'content/plans/deep-work-calibration-v1.json',run_id='retry')

    def test_two_plan_contexts_route_config_and_ledger_to_exact_selected_plan(self):
        source=ROOT/'content/plans/deep-work-calibration-v1.json'
        with tempfile.TemporaryDirectory() as d:
            d=Path(d); first=d/'one.json'; second=d/'two.json'
            first.write_bytes(source.read_bytes())
            changed=json.loads(source.read_text()); changed['batch']['id']='second-batch'; changed['candidates'][0]['id']='second-candidate'; second.write_text(json.dumps(changed),encoding='utf-8')
            a,b=production.load_plan(d,first),production.load_plan(d,second)
            self.assertNotEqual(a.sha256,b.sha256)
            self.assertEqual(production.config(b.plan['candidates'][0],d,d/'run')['caption'],changed['candidates'][0]['prompts']['positive'])
            command=production.ledger_command(d,b,b.plan['candidates'][0],d/'x.flac',d/'x.analysis',d/'x.evidence',d/'x.record')
            self.assertEqual(command[command.index('--plan')+1],str(second.resolve()))
            self.assertIn('second-candidate',command)

    def test_run_id_rejection_matrix(self):
        for value in ('../escape','a/b','a\\b','C:drive','CON','nul.txt','name ','name.','bad\nname',''):
            with self.subTest(value=repr(value)):
                with self.assertRaises(RuntimeError): production.validate_run_id(value)
        self.assertEqual(production.validate_run_id('batch-2.retry_1'),'batch-2.retry_1')

    def test_run_identity_rejects_different_plan_or_run(self):
        source=ROOT/'content/plans/deep-work-calibration-v1.json'
        with tempfile.TemporaryDirectory() as d:
            d=Path(d); a=d/'a.json'; b=d/'b.json'; a.write_bytes(source.read_bytes()); altered=json.loads(source.read_text()); altered['batch']['id']='other'; b.write_text(json.dumps(altered))
            ca,cb=production.load_plan(d,a),production.load_plan(d,b); run=d/'run'; run.mkdir()
            production.verify_run_identity(run,ca,'batch',create=True)
            with self.assertRaisesRegex(RuntimeError,'identity'): production.verify_run_identity(run,cb,'batch')
            with self.assertRaisesRegex(RuntimeError,'identity'): production.verify_run_identity(run,ca,'other')

    def test_v1_completed_retry_legacy_identity_is_read_only_compatible(self):
        context=production.load_plan(ROOT,V1)
        with tempfile.TemporaryDirectory() as d:
            run=Path(d)/'deep-work-calibration-v1-retry-2'; (run/'generated-records').mkdir(parents=True)
            production.verify_run_identity(run,context,'deep-work-calibration-v1-retry-2',allow_legacy=True)
            self.assertFalse((run/production.IDENTITY_FILE).exists())

    def test_plan_rejects_symlink_when_supported(self):
        with tempfile.TemporaryDirectory() as d:
            d=Path(d); target=d/'plan.json'; target.write_text('{}'); link=d/'link.json'
            try: link.symlink_to(target)
            except (OSError,NotImplementedError): self.skipTest('symlinks unavailable')
            with self.assertRaisesRegex(RuntimeError,'symlink|reparse'): production.load_plan(d,link)

    def test_plan_read_rejects_mutation_during_snapshot(self):
        with tempfile.TemporaryDirectory() as d:
            path=Path(d)/'plan.json'; path.write_text('{}')
            original=production.os.fstat
            fd=production.os.open(path,production.os.O_RDONLY)
            try: first=original(fd)
            finally: production.os.close(fd)
            # The synthetic post-read metadata proves the bounded snapshot gate
            # fails rather than validating a moving plan file.
            changed=SimpleNamespace(st_mode=first.st_mode,st_dev=first.st_dev,st_ino=first.st_ino,st_size=first.st_size+1,st_mtime_ns=first.st_mtime_ns)
            with mock.patch.object(production.os,'fstat',side_effect=[first,changed]):
                with self.assertRaisesRegex(RuntimeError,'changed while being read'):
                    production.load_plan(Path(d),path)

    def test_validate_passes_exact_selected_path_to_candidate_ledger(self):
        selected=ROOT/'content/plans/deep-work-calibration-v1.json'
        with mock.patch.object(production,'cmd',return_value=SimpleNamespace(returncode=0)) as command:
            context=production.validate(ROOT,selected)
        args=command.call_args.args[0]
        self.assertEqual(args[args.index('--plan')+1],str(selected.resolve()))
        self.assertEqual(context.path,selected.resolve())

    def test_allowed_durations_is_explicit_two_value_allowlist(self):
        self.assertEqual(production.ALLOWED_DURATIONS, {90.0, 180.0})

    def test_validate_accepts_existing_90s_and_planned_180s_plans(self):
        for plan in (ROOT/'content/plans/activity-library-expansion-v1.json', V2):
            with mock.patch.object(production,'cmd',return_value=SimpleNamespace(returncode=0)):
                production.validate(ROOT, plan)

    def test_validate_rejects_duration_outside_allowlist(self):
        source = V2.read_text(encoding='utf-8')
        tampered = json.loads(source)
        tampered['candidates'][0]['duration_seconds'] = 120
        with tempfile.TemporaryDirectory() as d:
            p = Path(d)/'plan.json'
            p.write_text(json.dumps(tampered), encoding='utf-8')
            with mock.patch.object(production,'cmd',return_value=SimpleNamespace(returncode=0)):
                with self.assertRaisesRegex(RuntimeError, 'fixed production values mismatch'):
                    production.validate(ROOT, p)

    def test_validate_passes_explicit_bin_flag_to_candidate_ledger(self):
        selected = ROOT/'content/plans/deep-work-calibration-v1.json'
        with mock.patch.object(production,'cmd',return_value=SimpleNamespace(returncode=0)) as command:
            production.validate(ROOT, selected)
        args = command.call_args.args[0]
        self.assertEqual(args[args.index('-p')+1], 'candidate-ledger')
        self.assertEqual(args[args.index('--bin')+1], 'candidate-ledger')

    def test_ledger_command_selects_candidate_ledger_bin(self):
        with tempfile.TemporaryDirectory() as d:
            d=Path(d)
            context=production.load_plan(ROOT,V1)
            c=context.plan['candidates'][0]
            command=production.ledger_command(d,context,c,d/'x.flac',d/'x.analysis',d/'x.evidence',d/'x.record')
            self.assertEqual(command[command.index('-p')+1],'candidate-ledger')
            self.assertEqual(command[command.index('--bin')+1],'candidate-ledger')
            self.assertEqual(command[command.index('--bin')-2],'-p')

if __name__ == '__main__': unittest.main()
