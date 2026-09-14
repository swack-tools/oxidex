"""Copied-native and actual writer differential for generated raw properties."""
from copy import deepcopy
import json
import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest

from raw_jfif_codegen import Refused, compile_fact, evaluate, generate

HERE=Path(__file__).resolve().parent
ROOT=HERE.parents[1]
PERL=os.environ.get('EXIFTOOL_PERL')
LIB=os.environ.get('OXIDEX_EXIFTOOL_LIB')
NATIVE=bool(PERL and LIB)

def env(): return {k:v for k,v in os.environ.items() if k not in {'PERL5LIB','PERLLIB','PERL5OPT'}}

def capture(library):
    return json.loads(subprocess.run([PERL,str(HERE/'capture_raw_jfif_fact.pl'),str(library)],env=env(),check=True,capture_output=True,text=True).stdout)

def base_jpeg():
    data=(ROOT/'tests/fixtures/jpeg/tag_matrix_base.jpg').read_bytes()
    assert data[:2]==b'\xff\xd8'
    out=bytearray(data[:2]);at=2
    while at<len(data):
        start=at;assert data[at]==255
        while data[at]==255:at+=1
        marker=data[at];at+=1
        if marker in (0xda,0xd9):out.extend(data[start:]);break
        if marker in (0,1) or 0xd0<=marker<=0xd7:out.extend(data[start:at]);continue
        size=int.from_bytes(data[at:at+2],'big');end=at+size
        if marker not in (0xe0,0xe1):out.extend(data[start:end])
        at=end
    return bytes(out)

NATIVE_PROBE=r'''
use strict; use warnings; use JSON::PP;
BEGIN { no warnings 'once'; $Image::ExifTool::configFile=''; }
use Image::ExifTool;
local $/; my $input=JSON::PP->new->decode(<STDIN>); my @results;
for my $row (@{$input->{cases}}) {
 my $et=Image::ExifTool->new; my $bytes=pack('H*',$row->{jpeg}); my $output='';
 $et->SetNewValue('IFD0:ImageDescription','raw source properties');
 my $result=$et->WriteInfo(\$bytes,\$output);
 my %properties;
 for my $key (@{$input->{properties}}) { $properties{$key}=0+$et->{$key} if exists($et->{$key}) && defined($et->{$key}); }
 push @results,{result=>$result,properties=>\%properties,error=>$et->GetValue('Error')};
}
print JSON::PP->new->canonical->encode(\@results);
'''

def native(library,recipe,cases):
    image=base_jpeg()
    rows=[]
    for marker,payload in cases:
        segment=bytes([255,marker])+(len(payload)+2).to_bytes(2,'big')+payload
        rows.append({'jpeg':(image[:2]+segment+image[2:]).hex()})
    result=subprocess.run([PERL,'-I'+str(library),'-e',NATIVE_PROBE],input=json.dumps({'cases':rows,'properties':[f.property for f in recipe.fields]}),env=env(),check=True,capture_output=True,text=True)
    results=json.loads(result.stdout)
    for result in results:
        if result['result']!=1:raise AssertionError(result)
    return [result['properties'] for result in results]


def rust(fact,cases):
    source,_=generate(fact,PERL)
    with tempfile.TemporaryDirectory() as directory:
        directory=Path(directory);generated=directory/'generated.rs';generated.write_text(source)
        lines=[]
        for marker,payload in cases:
            raw=','.join(str(b) for b in payload)
            lines.append(f'let p=decode_raw_segment(&writers::generated::RAW_JFIF,{marker},&[{raw}]).unwrap(); match p {{ None=>println!("NONE"), Some(p)=>{{ for (k,v) in p {{ print!("{{}}={{}};",k,v); }} println!(); }} }}')
        driver=directory/'main.rs'; binary=directory/'probe'
        runtime=ROOT/'src/writers/raw_segment_properties.rs'
        driver.write_text(f'''mod writers {{ #[path="{runtime}"] pub mod raw_segment_properties; #[path="{generated}"] pub mod generated; }}
use writers::raw_segment_properties::*;
fn main() {{ {''.join(lines)} }}''')
        subprocess.run(['rustc','--edition=2021',str(driver),'-o',str(binary)],check=True,capture_output=True,text=True)
        output=subprocess.run([str(binary)],check=True,capture_output=True,text=True).stdout.splitlines()
    return [None if line=='NONE' else {key:int(value) for key,value in (part.split('=',1) for part in line.split(';') if part)} for line in output]


CREATION_PROBE=r'''use strict;use warnings;use JSON::PP;BEGIN {no warnings 'once'; $Image::ExifTool::configFile='';} use Image::ExifTool;local $/;my $in=JSON::PP->new->decode(<STDIN>);my @rows;for my $row (@$in) {my $et=Image::ExifTool->new;my $bytes=pack('H*',$row->{jpeg});my $output='';$et->SetNewValue('IFD0:ImageDescription','timing probe');my $r=$et->WriteInfo(\$bytes,\$output);my %raw;for my $key(qw(JFIFResolutionUnit JFIFXResolution JFIFYResolution)) {$raw{$key}=$et->{$key} if exists $et->{$key};}my $rd=Image::ExifTool->new;my $info=$rd->ImageInfo(\$output,{PrintConv=>0,Duplicates=>1});my %density;for my $key(keys %$info) {next unless $key =~ /^(XResolution|YResolution|ResolutionUnit)(?: \(\d+\))?$/;my $name=$1;$density{$name}=$info->{$key} if $rd->GetGroup($key,1) eq 'IFD0';}push @rows,{name=>$row->{name},result=>$r,error=>scalar($et->GetValue('Error')),final_raw=>\%raw,ifd0=>\%density,output=>unpack('H*',$output)};}print JSON::PP->new->canonical->encode(\@rows);'''

def creation_cases():
    def seg(marker,payload):return bytes([255,marker])+(len(payload)+2).to_bytes(2,'big')+payload
    def jfif(u,x,y):return seg(224,b'JFIF\0\x01\x02'+bytes([u])+x.to_bytes(2,'big')+y.to_bytes(2,'big')+b'\0\0')
    a=jfif(1,72,96);b=jfif(2,300,600);p=seg(226,b'unrelated APP2')
    e=seg(225,b'Exif\0\0'+bytes.fromhex('49492a0008000000000000000000'))
    z=jfif(0,0,0);short=seg(224,b'JFIF\0\x01\x02\x02\x01\x2c')
    return {
        'fresh_a_b':([a,b],(300,600,3)),
        'fresh_a_p_b':([a,p,b],(72,96,2)),
        'fresh_p_a':([p,a],None),
        'existing_e_a':([e,a],None),
        'existing_a_e_b':([a,e,b],(72,96,2)),
        'existing_a_b_e':([a,b,e],(300,600,3)),
        'existing_p_a_e_b':([p,a,e,b],(72,96,2)),
        'existing_e_p_a':([e,p,a],None),
        'fresh_a_partial_b':([a,short],(300,96,3)),
        'fresh_a_zero':([a,z],(0,0,1)),
        'fresh_nonjfif_app0_a':([seg(224,b'other'),a],(72,96,2)),
    }

@unittest.skipUnless(NATIVE,'EXIFTOOL_PERL and OXIDEX_EXIFTOOL_LIB select canonical native source')
class RawJfifTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls): cls.fact=capture(LIB)

    def test_raw_zero_and_every_truncated_field_match_native_writer_and_rust(self):
        recipe=compile_fact(self.fact)
        payload=b'JFIF\0\x01\x02\x00\x00\x00\x00\x00\0\0'
        cases=[(recipe.marker,payload[:length]) for length in range(len(payload)+1)]
        cases += [(recipe.marker,b'JFIF\0\x01\x02'+bytes([unit])+b'\x01\x00\x00\x90\0\0') for unit in (0,1,2,3)]
        cases += [(0xe2,payload),(recipe.marker,b'NOPE\0'+payload[5:])]
        expected=[evaluate(recipe,*case) for case in cases]
        self.assertEqual(rust(self.fact,cases),expected)
        self.assertEqual(native(LIB,recipe,cases),[row or {} for row in expected])
        self.assertEqual(expected[len(payload)],{'JFIFResolutionUnit':0,'JFIFXResolution':0,'JFIFYResolution':0})

    def test_copied_offsets_types_properties_and_dispatch_byte_order_propagate(self):
        payload=b'JFIF\0\x01\x02\x01\x02\x03\x04\x05\x06\x07'
        mutations=(
            ('offset',lambda s:s.replace('DATAMEMBER => [ 2, 3, 5 ]','DATAMEMBER => [ 2, 4, 5 ]').replace("    3 => {\n        Name => 'XResolution'","    4 => {\n        Name => 'XResolution'")),
            ('type',lambda s:s.replace("Name => 'XResolution',\n        Format => 'int16u'","Name => 'XResolution',\n        Format => 'int8u'")),
            ('property',lambda s:s.replace('$$self{JFIFYResolution} = $val','$$self{NativeAlternateDensity} = $val')),
        )
        original=compile_fact(self.fact)
        for label,mutation in mutations:
            with self.subTest(mutation=label),tempfile.TemporaryDirectory() as directory:
                copied=Path(directory)/'lib';shutil.copytree(LIB,copied)
                source=copied/'Image/ExifTool.pm';body=source.read_text()
                start=body.index('%Image::ExifTool::JFIF::Main = (');end=body.index('%Image::ExifTool::JFIF::Extension',start)
                changed=mutation(body[start:end]);self.assertNotEqual(changed,body[start:end]);source.write_text(body[:start]+changed+body[end:])
                fact=capture(copied);recipe=compile_fact(fact)
                self.assertNotEqual(recipe.fields,original.fields)
                cases=[(recipe.marker,payload),(recipe.marker,payload[:11])]
                expected=[evaluate(recipe,*case) for case in cases]
                self.assertEqual(rust(fact,cases),expected)
                self.assertEqual(native(copied,recipe,cases),expected)
        with tempfile.TemporaryDirectory() as directory:
            copied=Path(directory)/'lib';shutil.copytree(LIB,copied)
            source=copied/'Image/ExifTool/Writer.pl';body=source.read_text()
            anchor="last unless $$editDirs{JFIF};\n                    SetByteOrder('MM');"
            self.assertEqual(body.count(anchor),1);source.write_text(body.replace(anchor,anchor.replace("'MM'","'II'")))
            fact=capture(copied);recipe=compile_fact(fact);self.assertEqual(recipe.byte_order,'II')
            cases=[(recipe.marker,payload)];expected=[evaluate(recipe,*cases[0])]
            self.assertEqual(rust(fact,cases),expected);self.assertEqual(native(copied,recipe,cases),expected)

        with tempfile.TemporaryDirectory() as directory:
            copied=Path(directory)/'lib';shutil.copytree(LIB,copied)
            source=copied/'Image/ExifTool/Writer.pl';body=source.read_text()
            before="DirStart => 5,     # directory starts after identifier\n                        DirLen   => $length-5,"
            start=body.index('last unless $$editDirs{JFIF};');end=body.index('} elsif',start)
            arm=body[start:end];self.assertEqual(arm.count(before),1)
            source.write_text(body[:start]+arm.replace(before,before.replace('=> 5,','=> 6,').replace('$length-5,','$length-6,'))+body[end:])
            fact=capture(copied);recipe=compile_fact(fact);self.assertEqual(recipe.skip,6)
            cases=[(recipe.marker,payload)];expected=[evaluate(recipe,*cases[0])]
            self.assertEqual(rust(fact,cases),expected);self.assertEqual(native(copied,recipe,cases),expected)

    def test_copied_executable_insertions_unknown_raw_controls_and_formats_refuse(self):
        mutations=(
            ('Image/ExifTool/Writer.pl',"last unless $$editDirs{JFIF};","last unless $$editDirs{JFIF}; $$segDataPt = substr($$segDataPt,1);"),
            ('Image/ExifTool.pm','my $maxLen = $dataLen - $dirStart;','my $maxLen = $dataLen - $dirStart; $dirStart += 1;'),
            ('Image/ExifTool.pm','$$self{JFIFYResolution} = $val','$$self{JFIFYResolution} = $val + 1'),
            ('Image/ExifTool.pm','$$self{JFIFYResolution} = $val','$$self{OPTIONS} = $val'),
            ('Image/ExifTool.pm',"Name => 'XResolution',\n        Format => 'int16u'","Name => 'XResolution',\n        Format => 'int32u'"),
        )
        for file,before,after in mutations:
            with self.subTest(mutation=before),tempfile.TemporaryDirectory() as directory:
                copied=Path(directory)/'lib';shutil.copytree(LIB,copied)
                source=copied/file;body=source.read_text();self.assertEqual(body.count(before),1)
                source.write_text(body.replace(before,after))
                with self.assertRaises(Refused):compile_fact(capture(copied))

    def test_duplicate_jfif_creation_and_existing_empty_exif_property_timing(self):
        cases=creation_cases();image=base_jpeg()
        rows=[{'name':name,'jpeg':(image[:2]+b''.join(segments)+image[2:]).hex()} for name,(segments,_) in cases.items()]
        cp=subprocess.run([PERL,'-I'+str(LIB),'-e',CREATION_PROBE],input=json.dumps(rows),env=env(),check=True,capture_output=True,text=True)
        for row in json.loads(cp.stdout):
            with self.subTest(case=row['name']):
                self.assertEqual(row['result'],1,row)
                expected=cases[row['name']][1]
                self.assertEqual({key:int(value) for key,value in row['ifd0'].items()},{} if expected is None else dict(zip(('XResolution','YResolution','ResolutionUnit'),expected)))

    def test_creation_operands_and_unknown_placement_controls_refuse(self):
        recipe=compile_fact(self.fact)
        self.assertEqual(recipe.creation_skip_markers,(224,))
        self.assertEqual(recipe.creation_wait_for_directories,('IFD0','ExtendedEXIF'))
        self.assertEqual(recipe.creation_timing,'BeforeCurrentSegment')
        changed=deepcopy(self.fact);changed['marker_names']['entries']['224']='APP2'
        with self.assertRaises(Refused):compile_fact(changed)
        changed=deepcopy(self.fact);changed['marker_names']['source']['sha256']='0'*64
        with self.assertRaises(Refused):compile_fact(changed)
        mutations=(
            ('Image/ExifTool/Writer.pl',"last if $markerName eq 'APP0' or $dirCount{IFD0}","last if $markerName eq 'APP2' or $dirCount{IFD0}"),
            ('Image/ExifTool.pm',"0xe0 => 'APP0',", "0xe0 => 'APP2',"),
            ('Image/ExifTool.pm','my $markerName = $jpegMarker{$marker};','my $markerName = $jpegMarker{$marker}; $markerName = "APP0";'),
        )
        for file,before,after in mutations:
            with self.subTest(mutation=before),tempfile.TemporaryDirectory() as directory:
                copied=Path(directory)/'lib';shutil.copytree(LIB,copied)
                source=copied/file;body=source.read_text();self.assertEqual(body.count(before),1)
                source.write_text(body.replace(before,after))
                with self.assertRaises(Refused):compile_fact(capture(copied))

    def test_each_native_body_and_source_join_is_required(self):
        for name in self.fact['functions']:
            with self.subTest(function=name):
                changed=deepcopy(self.fact);changed['functions'][name]['body']+='\n($val += 1);'
                with self.assertRaises(Refused):compile_fact(changed)
                changed=deepcopy(self.fact);changed['functions'][name]['source']['sha256']='0'*64
                with self.assertRaises(Refused):compile_fact(changed)
        changed=deepcopy(self.fact);changed['table']['entries']['3']['Condition']='1'
        with self.assertRaises(Refused):compile_fact(changed)
        changed=deepcopy(self.fact);changed['table']['entries']['3']['RawConv']='$$self{X} = $val; $val += 1;'
        with self.assertRaises(Refused):compile_fact(changed)

    def test_native_prescan_must_join_dispatch_and_report_has_no_local_paths(self):
        changed=deepcopy(self.fact)
        changed['functions']['WriteJPEG']['body']=changed['functions']['WriteJPEG']['body'].replace("($segType = 'JFIF');", "($segType = 'JFIF'); ($length += 1);")
        with self.assertRaises(Refused):compile_fact(changed)
        source,report=generate(self.fact,PERL)
        self.assertNotIn(str(LIB),json.dumps(report));self.assertNotIn(str(PERL),json.dumps(report))
        self.assertIn(self.fact['loaded_closure']['Image/ExifTool.pm'],source)
        from artifacts import ARTIFACTS
        self.assertEqual({a.key for a in ARTIFACTS if a.producer=='raw_jfif_codegen'},{'raw-jfif-rules','raw-jfif-ledger'})

if __name__=='__main__':unittest.main()
