import unittest
from datetime import datetime,timezone
from lib.resource_metrics import normalize,CPU,MEMORY

class MetricsTests(unittest.TestCase):
    def test_invalid_worker_does_not_discard_healthy_worker(self):
        rows=[]
        for instance in ('1','2'):
            for metric in (CPU,MEMORY):
                rows.append({'resource':{'labels':{'instance_id':instance,'project_id':'p','zone':'z'}},
                    'metric':{'type':metric,'labels':{'state':'used'}},
                    'points':[{'interval':{'endTime':datetime.fromtimestamp(t,timezone.utc).isoformat()},
                        'value':{'doubleValue':float('nan') if instance=='1' else (.2 if metric==CPU else 20)}}
                        for t in (700,760,820,880)]})
        self.assertEqual(normalize(rows,[('1','p','z'),('2','p','z')],1000),[None,(.2,.2)])

    def test_malformed_point_does_not_discard_healthy_worker(self):
        rows=[]
        for instance in ('1','2'):
            for metric in (CPU,MEMORY):
                rows.append({'resource':{'labels':{'instance_id':instance,'project_id':'p','zone':'z'}},
                    'metric':{'type':metric,'labels':{'state':'used'}},
                    'points':[{'interval':{'endTime':datetime.fromtimestamp(t,timezone.utc).isoformat()},
                        'value':{'doubleValue':'malformed' if instance=='1' else (.2 if metric==CPU else 20)}}
                        for t in (700,760,820,880)]})
        self.assertEqual(normalize(rows,[('1','p','z'),('2','p','z')],1000),[None,(.2,.2)])
