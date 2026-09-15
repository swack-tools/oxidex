import copy
import json
from pathlib import Path
import unittest
import quicktime_userdata_specs as compiler


def snapshot():
    return json.loads(compiler.SNAPSHOT.read_text())


class UserDataCompilerTests(unittest.TestCase):
    def test_complete_safe_explicit_format_class(self):
        r=compiler.compile_document(snapshot())
        self.assertEqual(r['identity_counts'],{'source_records':213,'generated':17,'omitted':196})
        self.assertEqual(sum(x['source_format']['kind']=='string' for x in r['specs']),13)
        self.assertEqual(sum(x['source_format']['kind']=='unsigned' for x in r['specs']),4)
        self.assertEqual(sum('unsupported_userdata_implicit_format' in x['reasons'] and len(x['reasons'])==1 for x in r['ledger']),82)

    def test_declarative_new_id_name_width_group_enum_priority_propagate(self):
        d=snapshot();table=d['modules']['QuickTime']['tables']['UserData']
        table['tags']['new!']={'Name':'FutureCounter','Format':'int32u','Avoid':'1','Groups':{'1':'FutureGroup'},'PrintConv':{'kind':'enum','map':{'42':'Answer'},'directives':None}}
        table['tag_count']+=1
        r=compiler.compile_document(d);s=next(s for s in r['specs'] if s['name']=='FutureCounter')
        self.assertEqual(s['raw_fourcc'],'6e657721');self.assertEqual(s['source_format'],{'kind':'unsigned','width':32});self.assertEqual(s['priority'],0)
        self.assertEqual(s['group'],'FutureGroup');self.assertIn('Answer',compiler.render_rust(r))

    def test_uint64_refuses_until_full_json_number_representation_is_available(self):
        d=snapshot();d['modules']['QuickTime']['tables']['UserData']['tags']['CNCV']['Format']='int64u'
        r=compiler.compile_document(d)
        row=next(x for x in r['ledger'] if x['identity']['raw_key']=='CNCV')
        self.assertEqual(row['reasons'],['unsupported_userdata_uint64_json_representation'])

    def test_unknown_row_controls_refuse(self):
        for key,value in [('Count','2'),('RawConv','$val += 1'),('Condition','1'),('IText','1'),('FutureControl','1')]:
            with self.subTest(key=key):
                d=snapshot();d['modules']['QuickTime']['tables']['UserData']['tags']['CNCV'][key]=value
                r=compiler.compile_document(d);row=next(x for x in r['ledger'] if x['identity']['raw_key']=='CNCV')
                self.assertFalse(row['generated']);self.assertTrue(row['reasons'])

    def test_inserted_processor_statement_cannot_keep_old_semantics(self):
        d=snapshot();d['modules']['QuickTime']['tables']['UserData']['meta']['PROCESS_PROC']['__deparse']+='\n$val += 1;'
        self.assertEqual(compiler.compile_document(d)['specs'],[])

    def test_every_helper_changed_statement_or_binding_refuses(self):
        d=snapshot()
        for name in d['quicktime_userdata_reader_protocol']['dependencies']:
            for field,value in [('__deparse','\n$val += 1;'),('source_sha256','0'*64),('__name','wrong')]:
                with self.subTest(name=name,field=field):
                    changed=copy.deepcopy(d);f=changed['quicktime_userdata_reader_protocol']['dependencies'][name]
                    f[field]=f[field]+value if field=='__deparse' else value
                    self.assertEqual(compiler.compile_document(changed)['specs'],[])

    def test_callers_and_hasdata_start_unknown_controls_refuse(self):
        for key,value in [('HasData','1'),('Start','4'),('FutureControl','1')]:
            d=snapshot();d['quicktime_userdata_reader_protocol']['movie_userdata_edge']['SubDirectory'][key]=value
            self.assertEqual(compiler.compile_document(d)['specs'],[])
        for name in ('Main','Movie'):
            d=snapshot();d['quicktime_userdata_reader_protocol']['caller_processors'][name]['__deparse']+='\n$val += 1;'
            self.assertEqual(compiler.compile_document(d)['specs'],[])
        d=snapshot();d['quicktime_userdata_reader_protocol']['caller_meta']['Movie']['FORMAT']='int32u'
        self.assertEqual(compiler.compile_document(d)['specs'],[])

    def test_mixed_caller_source_is_refused(self):
        d=snapshot();d['quicktime_userdata_reader_protocol']['caller_processors']['Movie']['source_sha256']='0'*64
        self.assertEqual(compiler.compile_document(d)['specs'],[])

    def test_map_operands_compile_but_stale_map_digest_refuses(self):
        d=snapshot();m=d['quicktime_userdata_reader_protocol']['charset_map'];m['operands']['142']=0x2603
        self.assertEqual(compiler.compile_document(d)['specs'],[])
        m['map_sha256']=compiler.digest(m['operands']);m['source_sha256']='1'*64
        r=compiler.compile_document(d)
        self.assertEqual(len(r['specs']),17);self.assertIn('9731',compiler.render_rust(r))
        m['operands']['142']={'future':'control'};m['map_sha256']=compiler.digest(m['operands'])
        self.assertEqual(compiler.compile_document(d)['specs'],[])

    def test_default_charset_changes_are_explicitly_refused(self):
        d=snapshot();d['quicktime_userdata_reader_protocol']['default_charset']='UTF16'
        self.assertEqual(compiler.compile_document(d)['specs'],[])

    def test_generated_artifacts_replay(self):
        r=compiler.compile_document(snapshot())
        self.assertEqual(json.loads(compiler.LEDGER.read_text()),r)
        from verify_quicktime_reader import rust_matches
        self.assertTrue(rust_matches(compiler.render_rust(r),compiler.RUST.read_text()))

if __name__=='__main__':unittest.main()
