import copy,json,unittest
from pathlib import Path
import quicktime_keys_specs as specs
HERE=Path(__file__).resolve().parent
def snapshot(): return json.loads((HERE/'fixtures/quicktime_source_13_59.json').read_text())
class KeysSpecsTests(unittest.TestCase):
 def test_committed_direct_keys_specs_and_omissions_are_complete(self):
  r=specs.compile_document(snapshot())
  self.assertEqual(r['identity_counts'],{'source_records':81,'generated':70,'omitted':11})
  self.assertEqual(specs.serialized(r),(HERE/'quicktime_generated_keys_ledger.json').read_text())
  self.assertEqual(specs.render_rust(r),(specs.ROOT/'src/parsers/quicktime/generated_keys_specs.rs').read_text())
  bad={x['identity']['raw_key'] for x in r['ledger'] if not x['generated']}
  self.assertEqual(bad,{'creation_time','creationdate','detected-face','detected-face.bounds','live-photo-info','location.ISO6709','location.date','scene-illuminance','sdpd','setu','smartstyle-info'})
 def test_new_literal_key_is_generated_without_alias_mapping(self):
  d=snapshot();t=d['modules']['QuickTime']['tables']['Keys'];t['tags']['future.source']={'Name':'FutureSource'};t['tag_count']+=1
  r=specs.compile_document(d); self.assertIn('future.source',{x['source_key'] for x in r['specs']});self.assertIn('name: "FutureSource"',specs.render_rust(r))
 def test_changed_processkeys_body_refuses_all_direct_keys(self):
  d=snapshot();d['modules']['QuickTime']['tables']['Keys']['meta']['PROCESS_PROC']['__deparse']+=' changed';r=specs.compile_document(d)
  self.assertEqual(r['specs'],[]);self.assertTrue(all('missing_or_changed_processor_contract:PROCESS_PROC' in x['reasons'] for x in r['ledger']))
if __name__=='__main__':unittest.main()
