import unittest
from types import SimpleNamespace
from lib.worker_selection import rank_workers

class WorkerSelectionTests(unittest.TestCase):
    def test_named_exception_is_selected_only_at_approved_instance_id(self):
        import json
        from unittest.mock import patch
        from lib.worker_selection import select_worker
        approved={'name':'approved-bench','id':'2','project':'project','zone':'z','enabled':True}
        inventory=[{'name':'oxidex-runners-a','id':'1','zone':'zones/z','status':'RUNNING'},
                   {'name':'approved-bench','id':'2','zone':'zones/z','status':'RUNNING'}]
        with patch('lib.worker_selection.approved_instances',return_value=[approved]), \
             patch('lib.worker_selection.subprocess.check_output',return_value=json.dumps(inventory)), \
             patch('lib.worker_selection.read_utilization',return_value=[(.2,.2)]) as metrics, \
             patch('lib.worker_selection.subprocess.run',return_value=SimpleNamespace(returncode=0)) as ssh:
            vm,_=select_worker('project')
            self.assertEqual(vm.name,'approved-bench')
            self.assertEqual([v.instance_id for v in metrics.call_args.args[1]],['2'])
            inventory[1]['id']='3'
            metrics.reset_mock();ssh.reset_mock()
            with patch('lib.worker_selection.subprocess.check_output',return_value=json.dumps(inventory)):
                with self.assertRaisesRegex(RuntimeError,'No available'):
                    select_worker('project')
            metrics.assert_not_called();ssh.assert_not_called()

    def test_only_dedicated_builder_vms_reach_metrics_or_ssh(self):
        import json
        from unittest.mock import patch
        from lib.worker_selection import select_worker
        inventory=[{'name':'oxidex-runners-a','id':'1','zone':'zones/z','status':'RUNNING'},
                   {'name':'oxidex-buildbench-b','id':'2','zone':'zones/z','status':'RUNNING'},
                   {'name':'builder-fast','id':'3','zone':'zones/z','status':'RUNNING'}]
        with patch('lib.worker_selection.subprocess.check_output',return_value=json.dumps(inventory)), \
             patch('lib.worker_selection.read_utilization',return_value=[(.2,.2)]) as metrics, \
             patch('lib.worker_selection.subprocess.run',return_value=SimpleNamespace(returncode=0)) as ssh:
            vm,_=select_worker('project')
        self.assertEqual(vm.name,'builder-fast')
        self.assertEqual([candidate.name for candidate in metrics.call_args.args[1]],['builder-fast'])
        self.assertEqual(ssh.call_count,1)
        self.assertIn('builder-fast',ssh.call_args.args[0])

    def test_rank_by_bottleneck_not_cpu_only(self):
        rows=[(SimpleNamespace(name='memory-hot'), (.1,.7)),
              (SimpleNamespace(name='idle'),(.2,.3)),
              (SimpleNamespace(name='busy'),(.8,.1))]
        self.assertEqual([vm.name for vm,_ in rank_workers(rows)],['idle','memory-hot'])

    def test_missing_and_threshold_samples_are_excluded(self):
        rows=[(SimpleNamespace(name='missing'),None),(SimpleNamespace(name='limit'),(.75,.2))]
        self.assertEqual(rank_workers(rows),[])

    def test_busy_candidate_falls_back_to_next_worker(self):
        import json
        from unittest.mock import patch
        from lib.worker_selection import select_worker
        inventory=[{'name':'builder-a','id':'1','zone':'zones/z','status':'RUNNING'},
                   {'name':'builder-b','id':'2','zone':'zones/z','status':'RUNNING'}]
        with patch('lib.worker_selection.subprocess.check_output',return_value=json.dumps(inventory)), \
             patch('lib.worker_selection.read_utilization',return_value=[(.1,.1),(.2,.2)]), \
             patch('lib.worker_selection.subprocess.run',side_effect=[SimpleNamespace(returncode=75),SimpleNamespace(returncode=0)]):
            vm,sample=select_worker('project')
        self.assertEqual(vm.name,'builder-b')

    def test_capacity_wins_over_lower_utilization_percentage(self):
        rows=[(SimpleNamespace(name='small',cpus=4,memory_gib=16),(.1,.1)),
              (SimpleNamespace(name='large',cpus=32,memory_gib=128),(.4,.4))]
        self.assertEqual(rank_workers(rows)[0][0].name,'large')

    def test_probe_requires_executable_protocol_and_bounds_hang(self):
        import json
        import subprocess
        from unittest.mock import patch
        from lib.worker_selection import LAUNCHER_SHA256, select_worker
        inventory=[{'name':'builder-a','id':'1','zone':'zones/z','status':'RUNNING'},
                   {'name':'builder-b','id':'2','zone':'zones/z','status':'RUNNING'}]
        with patch('lib.worker_selection.subprocess.check_output',return_value=json.dumps(inventory)), \
             patch('lib.worker_selection.read_utilization',return_value=[(.1,.1),(.2,.2)]), \
             patch('lib.worker_selection.subprocess.run',side_effect=[subprocess.TimeoutExpired('ssh',60),SimpleNamespace(returncode=0)]) as run:
            vm,_=select_worker('project')
        self.assertEqual(vm.name,'builder-b')
        self.assertEqual(run.call_count,2)
        args=run.call_args.args[0]
        command=next(x for x in args if x.startswith('--command='))
        for required in ('test -x /usr/local/bin/oxidex-remote-build',LAUNCHER_SHA256,
                         'invalid.project prepare','Project must be a simple identifier'):
            self.assertIn(required,command)
        self.assertIn('--ssh-flag=-oServerAliveInterval=15',args)
        self.assertIn('--ssh-flag=-oServerAliveCountMax=3',args)
        self.assertEqual(run.call_args.kwargs['timeout'],60)

    def test_launcher_probe_checks_real_executable_and_protocol(self):
        import hashlib
        import subprocess
        import tempfile
        from pathlib import Path
        from lib.worker_selection import launcher_probe
        with tempfile.TemporaryDirectory() as directory:
            root=Path(directory)
            config=root/'config';config.write_text('{}')
            launcher=root/'launcher'
            launcher.write_text('#!/bin/sh\necho "ValueError: Project must be a simple identifier, never a path" >&2\nexit 1\n')
            launcher.chmod(0o755)
            digest=hashlib.sha256(launcher.read_bytes()).hexdigest()
            probe=launcher_probe(str(launcher),str(config),str(root/'draining'),digest,sudo='')
            self.assertEqual(subprocess.run(['sh','-c',probe]).returncode,0)
            launcher.chmod(0o644)
            self.assertNotEqual(subprocess.run(['sh','-c',probe]).returncode,0)
            launcher.chmod(0o755)
            launcher.write_text('#!/bin/sh\nexit 1\n')
            self.assertNotEqual(subprocess.run(['sh','-c',probe]).returncode,0)

    def test_workers_share_one_metric_query_and_missing_host_does_not_block_others(self):
        import json
        from unittest.mock import patch
        from lib.worker_selection import select_worker
        inventory=[{'name':'builder-a','id':'1','zone':'zones/z','status':'RUNNING'},
                   {'name':'builder-b','id':'2','zone':'zones/z','status':'RUNNING'}]
        with patch('lib.worker_selection.subprocess.check_output',return_value=json.dumps(inventory)), \
             patch('lib.worker_selection.read_utilization',return_value=[None,(.2,.2)]) as metrics, \
             patch('lib.worker_selection.subprocess.run',return_value=SimpleNamespace(returncode=0)):
            vm,_=select_worker('project')
        self.assertEqual(vm.name,'builder-b')
        metrics.assert_called_once()
        self.assertEqual([vm.instance_id for vm in metrics.call_args.args[1]],['1','2'])
