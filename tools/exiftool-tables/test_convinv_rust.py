"""Native ConvInv scalar tail versus freshly generated Rust, direct helper scope."""
import json, os, shutil
from pathlib import Path
import subprocess, tempfile, unittest
import checkexif_rust_codegen as check_codegen
import convinv_rust_codegen as conv_codegen
from checkexif_native_differential import resolve_library, resolve_perl
from scalar_helper_codegen import rust_string

ROOT=Path(__file__).resolve().parents[2]
NATIVE=r'''BEGIN{$Image::ExifTool::configFile=''}use strict;use warnings;use JSON::PP;use Encode();use Image::ExifTool;use Image::ExifTool::Exif;require 'Image/ExifTool/Writer.pl';my$e=Image::ExifTool->new;my$h=$e->GetTagInfo(\%Image::ExifTool::Exif::Main,0x013c)or die'HostComputer';my@o;for my$r(@{JSON::PP->new->utf8->decode(do{local$/;<STDIN>})}){my%x=%$h;$x{Count}=3;my$v=$r->{u}?undef:$r->{utf8}?$r->{v}:pack('H*',$r->{v});utf8::upgrade($v)if$r->{utf8};my($z,$err)=$e->ConvInv($v,\%x,'HostComputer','IFD0');my$u=defined$z&&utf8::is_utf8($z);push@o,{name=>$r->{name},defined=>defined$z?JSON::PP::true:JSON::PP::false,utf8=>$u?JSON::PP::true:JSON::PP::false,hex=>defined$z?unpack('H*',$u?Encode::encode('UTF-8',$z):$z):undef,error=>$err};}print JSON::PP->new->canonical->encode(\@o);'''

def scalar(case):
 if case.get('u'): return 'Scalar::Undefined'
 if case.get('utf8'): return 'Scalar::Utf8('+rust_string(case['v'])+'.to_owned())'
 return 'Scalar::Bytes(vec!['+','.join(str(x) for x in bytes.fromhex(case['v']))+'])'
def expected(row):
 value='Scalar::Undefined' if not row['defined'] else ('Scalar::Utf8('+rust_string(bytes.fromhex(row['hex']).decode())+'.to_owned())' if row['utf8'] else 'Scalar::Bytes(vec!['+','.join(str(x) for x in bytes.fromhex(row['hex']))+'])')
 error='None' if row['error'] is None else 'Some('+rust_string(row['error'])+'.to_owned())'
 return value,error
def rust_matches(test,cases,native,rules,conv_rules,directory):
 d=Path(directory);d.mkdir();rp=d/'rules.rs';rp.write_text(rules);cp=d/'conv_rules.rs';cp.write_text(conv_rules);src=ROOT/'src/writers'
 lines=['#![allow(dead_code)]',f'#[path={rust_string(str(ROOT/"src/error/mod.rs"))}]mod error;','mod writers{',f'#[path={rust_string(str(src/"generated_scalar.rs"))}]pub(crate)mod generated_scalar;',f'#[path={rust_string(str(src/"generated_checkexif.rs"))}]pub(crate)mod generated_checkexif;',f'#[path={rust_string(str(src/"generated_convinv.rs"))}]pub(crate)mod generated_convinv;}}',f'#[path={rust_string(str(rp))}]mod rules;',f'#[path={rust_string(str(cp))}]mod conv_rules;','use writers::generated_scalar::*;use writers::generated_checkexif::*;use writers::generated_convinv::*;','#[test]fn proof(){let recipe=rules::CHECK_EXIF_RECIPES.iter().find(|r|r.source_tables.iter().any(|t|t.full_name=="Image::ExifTool::Exif::Main")).unwrap();']
 for case,row in zip(cases,native,strict=True):
  val,err=expected(row);lines += ['let input=CheckExifInput{tag_properties:&[Property{name:"Writable",value:PropertyValue::Text("string")},Property{name:"Count",value:PropertyValue::Integer(3)}],table_properties:&[],tag_groups:&[]};',f'let row=ConvInvRow{{print_conv:ConversionProperty::Absent,print_conv_inv:ConversionProperty::Absent,value_conv:ConversionProperty::Absent,value_conv_inv:ConversionProperty::Absent,list:false,raw_join:false,write_check:false,raw_conv_inv:false,check_proc:Some((recipe,&input)),requested_tag:"HostComputer",actual_tag:"HostComputer",write_group:"IFD0"}};let got=conv_inv_scalar(&conv_rules::CONV_INV_RECIPE,'+scalar(case)+',row,None,None).unwrap();',f'assert_eq!(got.value,{val},{rust_string(case["name"])});assert_eq!(got.error,{err},{rust_string(case["name"]+" error")});']
 lines.append('}');proof=d/'proof.rs';proof.write_text('\n'.join(lines));built=subprocess.run(['rustc','--edition=2024','--test',str(proof),'-o',str(d/'proof')],capture_output=True,text=True,timeout=60);test.assertEqual(built.returncode,0,built.stderr);ran=subprocess.run([str(d/'proof')],capture_output=True,text=True,timeout=30);test.assertEqual(ran.returncode,0,ran.stderr)

@unittest.skipUnless(os.environ.get('EXIFTOOL_PERL') and os.environ.get('OXIDEX_PINNED_EXIFTOOL'),'requires explicit canonical Perl and pinned library')
class ConvInvRustTest(unittest.TestCase):
 def test_copied_source_separator_reaches_generated_conv_recipe(self):
  perl=resolve_perl(Path(os.environ['EXIFTOOL_PERL']));lib=resolve_library(Path(os.environ['OXIDEX_PINNED_EXIFTOOL']))
  env={k:v for k,v in os.environ.items() if k not in {'PERL5LIB','PERLLIB','PERL5OPT'}}
  with tempfile.TemporaryDirectory() as d:
   changed=Path(d)/'lib';shutil.copytree(lib,changed);source=changed/'Image/ExifTool/Writer.pl';text=source.read_text();start=text.index('sub ConvInv($$$$$;$$)\n{');end=text.index('\n#------------------------------------------------------------------------------',start);body=text[start:end];needle='$err = "$err2 for $wgrp1:$tag";';self.assertEqual(body.count(needle),1);source.write_text(text[:start]+body.replace(needle,'$err = "$err2 at $wgrp1:$tag";')+text[end:])
   doc=json.loads(subprocess.run([str(perl),str(ROOT/'tools/exiftool-tables/dump_tables.pl'),str(changed),'Exif'],env=env,capture_output=True,check=True,timeout=60).stdout); emitted,report=conv_codegen.generate(doc);check_rules,check_report=check_codegen.generate(doc);self.assertTrue(report['emitted']);self.assertTrue(check_report['recipes']);self.assertEqual(report['error_separator'],' at ');self.assertIn('error_separator:" at "',emitted)
   cases=[{'name':'bytes_nul_limit','v':'610062'},{'name':'utf8_padding','v':'é','utf8':1},{'name':'empty','v':''},{'name':'undef','u':1},{'name':'too_long','v':'61626364'}]
   out=json.loads(subprocess.run([str(perl),'-I'+str(changed),'-e',NATIVE],input=json.dumps(cases,ensure_ascii=False).encode(),env=env,capture_output=True,check=True,timeout=30).stdout);self.assertEqual(out[-1]['error'],'String too long at IFD0:HostComputer');rust_matches(self,cases,out,check_rules,emitted,Path(d)/'rust')

 def test_every_native_scalar_result_matches_fresh_generated_checkexif(self):
  perl=resolve_perl(Path(os.environ['EXIFTOOL_PERL']));lib=resolve_library(Path(os.environ['OXIDEX_PINNED_EXIFTOOL']))
  env={k:v for k,v in os.environ.items() if k not in {'PERL5LIB','PERLLIB','PERL5OPT'}}
  cases=[{'name':'bytes_nul_limit','v':'610062'},{'name':'utf8_padding','v':'é','utf8':1},{'name':'empty','v':''},{'name':'undef','u':1},{'name':'too_long','v':'61626364'}]
  native=json.loads(subprocess.run([str(perl),'-I'+str(lib),'-e',NATIVE],input=json.dumps(cases,ensure_ascii=False).encode(),env=env,capture_output=True,check=True,timeout=30).stdout)
  dumped=json.loads(subprocess.run([str(perl),str(ROOT/'tools/exiftool-tables/dump_tables.pl'),str(lib),'Exif'],env=env,capture_output=True,check=True,timeout=60).stdout)
  rules,report=check_codegen.generate(dumped);conv_rules,conv_report=conv_codegen.generate(dumped);self.assertTrue(report['recipes']);self.assertTrue(conv_report['emitted'])
  with tempfile.TemporaryDirectory() as d:
   d=Path(d);rp=d/'rules.rs';rp.write_text(rules);cp=d/'conv_rules.rs';cp.write_text(conv_rules)
   src=(ROOT/'src/writers');
   lines=['#![allow(dead_code)]',f'#[path={rust_string(str(ROOT/"src/error/mod.rs"))}]mod error;','mod writers{',f'#[path={rust_string(str(src/"generated_scalar.rs"))}]pub(crate)mod generated_scalar;',f'#[path={rust_string(str(src/"generated_checkexif.rs"))}]pub(crate)mod generated_checkexif;',f'#[path={rust_string(str(src/"generated_convinv.rs"))}]pub(crate)mod generated_convinv;}}',f'#[path={rust_string(str(rp))}]mod rules;',f'#[path={rust_string(str(cp))}]mod conv_rules;','use writers::generated_scalar::*;use writers::generated_checkexif::*;use writers::generated_convinv::*;','#[test]fn proof(){let recipe=rules::CHECK_EXIF_RECIPES.iter().find(|r|r.source_tables.iter().any(|t|t.full_name=="Image::ExifTool::Exif::Main")).unwrap();']
   for case,row in zip(cases,native,strict=True):
    val,err=expected(row);lines += ['let input=CheckExifInput{tag_properties:&[Property{name:"Writable",value:PropertyValue::Text("string")},Property{name:"Count",value:PropertyValue::Integer(3)}],table_properties:&[],tag_groups:&[]};',f'let row=ConvInvRow{{print_conv:ConversionProperty::Absent,print_conv_inv:ConversionProperty::Absent,value_conv:ConversionProperty::Absent,value_conv_inv:ConversionProperty::Absent,list:false,raw_join:false,write_check:false,raw_conv_inv:false,check_proc:Some((recipe,&input)),requested_tag:"HostComputer",actual_tag:"HostComputer",write_group:"IFD0"}};let got=conv_inv_scalar(&conv_rules::CONV_INV_RECIPE,'+scalar(case)+',row,None,None).unwrap();',f'assert_eq!(got.value,{val},{rust_string(case["name"])});assert_eq!(got.error,{err},{rust_string(case["name"]+" error")});']
   lines.append('}') ; proof=d/'proof.rs';proof.write_text('\n'.join(lines))
   built=subprocess.run(['rustc','--edition=2024','--test',str(proof),'-o',str(d/'proof')],capture_output=True,text=True,timeout=60);self.assertEqual(built.returncode,0,built.stderr)
   ran=subprocess.run([str(d/'proof')],capture_output=True,text=True,timeout=30);self.assertEqual(ran.returncode,0,ran.stderr)

if __name__=='__main__':unittest.main()
