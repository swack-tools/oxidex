"""Opt-in bounded copied-native capture mutation tests (no Cargo or corpus)."""
import copy
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest
import quicktime_userdata_specs as compiler

PERL=os.environ.get('EXIFTOOL_PERL')
LIB=os.environ.get('OXIDEX_PINNED_EXIFTOOL_LIB')

@unittest.skipUnless(PERL and LIB, 'explicit canonical Perl and pinned library required')
class CopiedNativeUserDataTests(unittest.TestCase):
    def setUp(self):
        self.temp=tempfile.TemporaryDirectory();self.addCleanup(self.temp.cleanup)
        self.lib=Path(self.temp.name)/'lib';shutil.copytree(LIB,self.lib)
        self.document=json.loads(compiler.SNAPSHOT.read_text())
        dump=compiler.ROOT/'tools/exiftool-tables/dump_tables.pl'
        prefix=dump.read_text().split('my @modules = @ARGV;',1)[0]
        self.script=Path(self.temp.name)/'capture.pl'
        self.script.write_text(prefix+'''require Image::ExifTool::QuickTime;
Image::ExifTool::GetTagTable('Image::ExifTool::QuickTime::UserData');
my $p = quicktime_userdata_reader_protocol_fact($EXIFTOOL_LIB_ABS);
my $proc = code_ref_fact($Image::ExifTool::QuickTime::UserData{PROCESS_PROC},
    'Image::ExifTool::QuickTime::ProcessMOV', $EXIFTOOL_LIB_ABS, undef, undef, 0);
my %row = %{$Image::ExifTool::QuickTime::UserData{CNCV}};
delete @row{'Table', 'TagID'};
print JSON::PP->new->utf8->canonical->encode({ protocol => $p, processor => $proc,
    row => dump_tag_entry(\\%row) });
''')

    def capture(self):
        run=subprocess.run([PERL,'-I'+str(compiler.ROOT/'tools/exiftool-tables'),str(self.script),'--reader-only',str(self.lib)],capture_output=True,check=True,timeout=10)
        fact=json.loads(run.stdout);d=copy.deepcopy(self.document)
        d['quicktime_userdata_reader_protocol']=fact['protocol']
        table=d['modules']['QuickTime']['tables']['UserData'];table['meta']['PROCESS_PROC']=fact['processor'];table['tags']['CNCV']=fact['row']
        return compiler.compile_document(d)

    def test_actual_native_capture_admits_complete_class(self):
        self.assertEqual(self.capture()['identity_counts']['generated'],17)

    def test_hydrated_catalog_keeps_detached_userdata_protocol_bounded(self):
        # GetTagTable during hydrated catalog capture attaches live Table
        # back-pointers. The protocol must be captured before that operation,
        # preserving the exact detached source edge used by the compiler.
        dump = compiler.ROOT / 'tools/exiftool-tables/dump_tables.pl'
        run = subprocess.run([
            PERL, str(dump), '--reader-only', '--hydrated-layouts',
            '--hydrated-layout-table', 'Image::ExifTool::QuickTime::UserData',
            str(LIB), 'QuickTime',
        ], capture_output=True, check=True, timeout=20)
        fact = json.loads(run.stdout)['quicktime_userdata_reader_protocol']
        self.assertEqual(fact['caller_meta'], {
            'Main': {'GROUPS': {'2': 'Video'}},
            'Movie': {'GROUPS': {'2': 'Video'}},
        })
        self.assertEqual(fact['main_movie_edge'], {
            'Name': 'Movie',
            'SubDirectory': {'TagTable': 'Image::ExifTool::QuickTime::Movie'},
        })
        self.assertEqual(fact['movie_userdata_edge'], {
            'Name': 'UserData',
            'SubDirectory': {'TagTable': 'Image::ExifTool::QuickTime::UserData'},
        })
        document = copy.deepcopy(self.document)
        document['quicktime_userdata_reader_protocol'] = fact
        self.assertEqual(compiler.compile_document(document)['identity_counts']['generated'], 17)

    def test_inserted_native_dispatch_assignment_refuses(self):
        p=self.lib/'Image/ExifTool/QuickTime.pm';s=p.read_text();needle="my $charsetQuickTime = $et->Options('CharsetQuickTime');"
        self.assertEqual(s.count(needle),1);p.write_text(s.replace(needle,needle+"\n    $charsetQuickTime = 'UTF16';"))
        r=self.capture();self.assertEqual(r['specs'],[]);self.assertEqual(r['protocol']['reason'],'unsupported_userdata_processor')

    def test_actual_native_format_change_propagates(self):
        p=self.lib/'Image/ExifTool/QuickTime.pm';s=p.read_text();start=s.index('    CNCV =>');end=s.index('\n',start)
        piece=s[start:end];self.assertEqual(piece.count("Format => 'string'"),1)
        p.write_text(s[:start]+piece.replace("Format => 'string'","Format => 'int16u'")+s[end:])
        r=self.capture();self.assertEqual(len(r['specs']),17)
        self.assertEqual(next(x for x in r['specs'] if x['raw_fourcc']=='434e4356')['source_format'],{'kind':'unsigned','width':16})

    def test_actual_native_charset_mapping_change_becomes_rust_operand(self):
        p=self.lib/'Image/ExifTool/Charset/MacRoman.pm';s=p.read_text();needle='0x8e => 0xe9'
        self.assertEqual(s.count(needle),1);p.write_text(s.replace(needle,'0x8e => 0x2603'))
        r=self.capture();self.assertEqual(len(r['specs']),17)
        self.assertEqual(r['protocol']['capture']['charset_map']['operands']['142'],0x2603)
        self.assertIn('9731',compiler.render_rust(r))

    def test_unknown_native_helper_statement_refuses_even_with_original_tokens(self):
        p=self.lib/'Image/ExifTool.pm';s=p.read_text();needle='sub IsUTF8($;$)\n{'
        self.assertEqual(s.count(needle),1);p.write_text(s.replace(needle,needle+'\n    return -1;'))
        r=self.capture();self.assertEqual(r['specs'],[]);self.assertIn('unsupported_userdata_helper',r['protocol']['reason'])

if __name__=='__main__':unittest.main()
