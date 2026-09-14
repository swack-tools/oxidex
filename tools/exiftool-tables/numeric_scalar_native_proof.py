#!/usr/bin/env python3
"""Bounded native/helper proof. Takes the shared gate lock; never runs Cargo.

Accepted bytes and rejected inputs are compared independently, including
decimal/float/negative and overflow boundaries. Public file-operation proof
belongs to the separate public matrix.
"""
import argparse
import copy
import fcntl
import hashlib
import json
from pathlib import Path
import shutil
import subprocess
import tempfile
import time

from numeric_scalar import compile_numeric_scalar, render_numeric
from checkexif_recipes import RecipeRefused
from test_numeric_scalar import source, ROOT
import native_write_matrix as native

PROGRAM = r'''
BEGIN { $Image::ExifTool::configFile = ''; }
use strict; use warnings; use JSON::PP; use B::Deparse; use Image::ExifTool;
require 'Image/ExifTool/Writer.pl';
''' + native.NATIVE_LIBRARY_GUARD + r'''
my $inputs = JSON::PP::decode_json(shift @ARGV);
my @rows;
for my $format ('int16u','rational64u') {
 for my $input (@$inputs) {
  for my $order ('II','MM') {
   Image::ExifTool::SetByteOrder($order);
   my $value = $input;
   my $error = Image::ExifTool::CheckValue(\$value,$format,1);
   my $bytes = defined $error ? undef : Image::ExifTool::WriteValue($value,$format,1);
   push @rows, {format=>$format,input=>$input,value=>$value,order=>$order,error=>$error,hex=>defined($bytes)?unpack('H*',$bytes):undef};
  }
 }
}
my $deparse = B::Deparse->new('-p','-sC')->coderef2text(\&Image::ExifTool::CheckValue);
print JSON::PP->new->canonical->encode({rows=>\@rows,check_body=>$deparse});
'''


def run(perl, lib, inputs):
    result = subprocess.run([str(perl), '-I'+str(lib), '-e', PROGRAM, str(lib), json.dumps(inputs)],
                            env=native.clean_env(), capture_output=True, text=True, timeout=30)
    result.check_returncode()
    return json.loads(result.stdout)


def proof(perl, lib, output):
    document = source()
    recipe = compile_numeric_scalar(document)
    inputs = ['0','1','258','300','65535','65536','4294967295','4294967296','3/2','1/0','0/0','1.5','1e3','-1','-1/2','1/-2','1,5','3.14159265358979','1e-10','1e-7','-0.49','-0.5','-0/1','+3/2','4294967296/1','1/4294967296','18446744073709551615/1','0xfeedfeed','face','+72','.5','65535.49','65535.5']
    observed = run(perl, lib, inputs)
    (output/'native.json').write_text(json.dumps(observed,indent=2)+'\n')
    checks = []
    for row in observed['rows']:
        expected = 'None' if row['hex'] is None else 'Some("'+row['hex']+'")'
        checks.append(f'check(recipe,"{row["format"]}","{row["input"]}",{str(row["order"]=="II").lower()},{expected});')
    driver = output/'native-runtime.rs'
    driver.write_text('''#![allow(dead_code)]
mod error {#[derive(Debug)] pub struct ExifToolError;impl ExifToolError {pub fn unsupported_format<T: Into<String>>(_: T)->Self {Self}}pub type Result<T>=std::result::Result<T,ExifToolError>;}
#[path="'''+str(ROOT/'src/writers/generated_scalar.rs')+'''"] mod generated_scalar;
use generated_scalar::*;
'''+render_numeric(recipe)+'''
fn check(recipe:&NumericScalarRecipe,format:&str,input:&str,little:bool,native:Option<&str>) {
 match serialize_numeric(recipe,&Scalar::Utf8(input.into()),format,little) {
  Ok(bytes)=>{let actual=bytes.iter().map(|byte|format!("{byte:02x}")).collect::<String>();assert_eq!(Some(actual.as_str()),native);println!("matched {format} {input} {little}");}
  Err(_)=>println!("unsupported {format} {input} {little} native_accepted={}",native.is_some()),
 }
}
fn main(){let recipe=NUMERIC_SCALAR.as_ref().unwrap();
'''+ '\n'.join(checks)+'\n}\n')
    binary=output/'native-runtime'
    subprocess.run(['rustc','--edition=2024',str(driver),'-o',str(binary)],check=True,capture_output=True,text=True)
    result=subprocess.run([str(binary)],check=True,capture_output=True,text=True)
    (output/'runtime.log').write_text(result.stdout)
    matched=sum(line.startswith('matched ') for line in result.stdout.splitlines())
    unsupported=[line for line in result.stdout.splitlines() if line.startswith('unsupported ')]
    with tempfile.TemporaryDirectory(prefix='oxidex-numeric-copied-source-') as directory:
        copied=Path(directory)/'lib';shutil.copytree(lib,copied)
        writer=copied/'Image/ExifTool/Writer.pl'
        body=writer.read_text();needle='my ($valPtr, $format, $count) = @_;';assert body.count(needle)==1
        writer.write_text(body.replace(needle,needle+'\n    $$valPtr = 301;'))
        mutated=run(perl,copied,['300'])
        (output/'copied-native.json').write_text(json.dumps(mutated,indent=2)+'\n')
        assert next(row['hex'] for row in observed['rows'] if row['format']=='int16u' and row['input']=='300' and row['order']=='II') != mutated['rows'][0]['hex']
        fact=copy.deepcopy(document)
        digest=hashlib.sha256(writer.read_bytes()).hexdigest()
        check=fact['native_write_helpers']['check_value'];check['__deparse']=mutated['check_body'];check['source_sha256']=digest
        # Change every Writer source join together, so refusal is executable
        # source recognition, never merely an old source hash mismatch.
        def rebind(value):
            if isinstance(value,dict):
                if value.get('source_file')=='Image/ExifTool/Writer.pl':value['source_sha256']=digest
                for item in value.values():rebind(item)
            elif isinstance(value,list):
                for item in value:rebind(item)
        rebind(fact)
        fact['native_write_capture_context']['loaded_modules']['Image/ExifTool/Writer.pl']=digest
        for item in fact['native_capture_context']['loaded_closure']['modules']:
            if item['inc']=='Image/ExifTool/Writer.pl':item['source_sha256']=digest
        try:compile_numeric_scalar(fact)
        except RecipeRefused as error:
            assert 'CheckValue statement grammar' in str(error), str(error)
            refusal=str(error)
        else:raise AssertionError('copied source insertion was accepted')
    report={'matched':matched,'unsupported':unsupported,'native_cases':len(observed['rows']),
            'copied_source_refusal':refusal,'public_file_write_proof':'not_run',
            'native_identity':native.native_identity(perl,lib)}
    (output/'report.json').write_text(json.dumps(report,indent=2)+'\n')
    return report


def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--perl',type=Path,required=True);parser.add_argument('--lib',type=Path,required=True)
    parser.add_argument('--lock',type=Path,required=True);parser.add_argument('--output',type=Path,required=True)
    args=parser.parse_args();args.output.mkdir(parents=True,exist_ok=True)
    status=args.output/'status.json'
    status.write_text(json.dumps({'state':'waiting-lock','started':time.time()})+'\n')
    try:
        with args.lock.open('a') as lock:
            fcntl.flock(lock,fcntl.LOCK_EX)
            status.write_text(json.dumps({'state':'running','started':time.time()})+'\n')
            report=proof(args.perl,args.lib,args.output)
        status.write_text(json.dumps({'state':'passed','matched':report['matched'],'finished':time.time()})+'\n')
    except Exception as error:
        status.write_text(json.dumps({'state':'failed','error':str(error),'finished':time.time()})+'\n')
        raise

if __name__=='__main__':main()
