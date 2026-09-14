import json,tempfile,unittest
from pathlib import Path
import verify_quicktime_keys_reader as v
HERE=Path(__file__).parent
class T(unittest.TestCase):
 def test_cases_have_distinct_authenticated_bytes(self):
  c=v.cases();self.assertEqual(len(c),9);self.assertEqual(set(c),{'prefix','full-retry','ordinal','count-ignored','utf8','numeric-enum','unknown','refused-gps','refused-date'})
 def test_report_rejects_forged_projection(self):
  row={'fixture':'x','native_json':'[{"SourceFile":"x","Keys:Artist":"Ada"}]','oxidex_json':'[{"SourceFile":"x","Keys:Artist":"Ada"}]','native_json_sha256':'','oxidex_json_sha256':'','expected':{'forged':'x'},'actual':{},'matched':True}
  row['native_json_sha256']=__import__('hashlib').sha256(row['native_json'].encode()).hexdigest();row['oxidex_json_sha256']=__import__('hashlib').sha256(row['oxidex_json'].encode()).hexdigest()
  with self.assertRaises(ValueError):v.validate_report({'observations':[row]},{'specs':[]})
 def test_source_replay_rejects_ledger_change(self):
  with tempfile.TemporaryDirectory() as d:
   d=Path(d);s=HERE/'fixtures/quicktime_source_13_59.json';l=HERE/'quicktime_generated_keys_ledger.json';r=Path(__file__).parents[2]/'src/parsers/quicktime/generated_keys_specs.rs'; bad=d/'ledger.json';x=json.loads(l.read_text());x['identity_counts']['generated']=0;bad.write_text(json.dumps(x))
   with self.assertRaises(ValueError):v.authenticated(s,bad,r)
if __name__=='__main__':unittest.main()
