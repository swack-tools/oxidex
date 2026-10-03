import unittest
from types import SimpleNamespace
from lib.worker_selection import rank_workers

class WorkerSelectionTests(unittest.TestCase):
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
        inventory=[{'name':'oxidex-runners-a','id':'1','zone':'zones/z','status':'RUNNING'},
                   {'name':'oxidex-runners-b','id':'2','zone':'zones/z','status':'RUNNING'}]
        with patch('lib.worker_selection.subprocess.check_output',return_value=json.dumps(inventory)), \
             patch('lib.worker_selection.read_utilization',side_effect=[[(.1,.1)],[(.2,.2)]]), \
             patch('lib.worker_selection.subprocess.run',side_effect=[SimpleNamespace(returncode=75),SimpleNamespace(returncode=0)]):
            vm,sample=select_worker('project')
        self.assertEqual(vm.name,'oxidex-runners-b')

    def test_capacity_wins_over_lower_utilization_percentage(self):
        rows=[(SimpleNamespace(name='small',cpus=4,memory_gib=16),(.1,.1)),
              (SimpleNamespace(name='large',cpus=32,memory_gib=128),(.4,.4))]
        self.assertEqual(rank_workers(rows)[0][0].name,'large')
