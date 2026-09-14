#!/usr/bin/env python3
"""Bounded UserData direct-format byte fixtures and pinned native transcripts.

Requires an explicit lock and canonical Perl/library. Native-only mode produces
portable decoder fixtures; --oxidex additionally compares the real public read
route. Native-only results never claim Rust runtime coverage.
"""
from __future__ import annotations
import argparse
import hashlib
import json
from pathlib import Path
import subprocess
import fcntl
import runtime_evidence_inputs as runtime_inputs
import quicktime_userdata_specs as compiler
from quicktime_baseline import atom


def cases(specs):
    rows=[]
    def add(label,s,hex_value,repeat=1,suffix=''):
        rows.append({'case':label,'raw_fourcc':s['raw_fourcc'],
                     'target':s['group']+':'+s['name'],
                     'payload':{'hex':hex_value,'repeat':repeat,'suffix_hex':suffix}})
    for s in specs:
        fmt=s['source_format']
        payload='c3a9' if fmt['kind']=='string' else (2).to_bytes(fmt['width']//8,'big').hex()
        add('declaration-'+s['raw_fourcc'],s,payload)
        if fmt['kind']=='unsigned':
            w=fmt['width']//8
            for label,payload in [('empty',''),('short','ff'*(w-1)),('max','ff'*w),
                                  ('array',(1).to_bytes(w,'big').hex()+(2).to_bytes(w,'big').hex()),
                                  ('partial-tail',(1).to_bytes(w,'big').hex()+'ff'*(w-1))]:
                add(s['raw_fourcc']+'-'+label,s,payload)
    strings=[s for s in specs if s['source_format']['kind']=='string']
    for bypass in (False,True):
        candidates=[s for s in strings if (bytes.fromhex(s['raw_fourcc'])[0]==169)==bypass]
        if not candidates:continue
        s=candidates[0]
        for label,payload in [('empty',''),('ascii','4142'),('nul','41008e'),('macroman','8e'),
                              ('invalid-lead','ff'),('overlong','c080'),('surrogate','eda080'),
                              ('noncharacter','efbfbe'),('scalar','f09f9880'),('partial-utf8','e282')]:
            add(s['raw_fourcc']+'-'+label,s,payload)
        for count in (65536,65537):add(s['raw_fourcc']+'-limit-'+str(count),s,'8e',count)
        add(s['raw_fourcc']+'-nul-before-limit',s,'8e',1,'00'+'8e'*65537)
    return rows


def payload(row):
    p=row['payload'];return bytes.fromhex(p['hex'])*p['repeat']+bytes.fromhex(p['suffix_hex'])

def fixture(row):
    return atom(b'ftyp',b'qt  \0\0\0\0qt  ')+atom(b'moov',atom(b'udta',atom(bytes.fromhex(row['raw_fourcc']),payload(row))))

def value_text(value):
    if isinstance(value,(str,int,float)) and not isinstance(value,bool):return str(value)
    raise ValueError('native target is not a scalar: '+repr(value))

def main():
    p=argparse.ArgumentParser(description=__doc__);p.add_argument('--perl',type=Path,required=True);p.add_argument('--lib',type=Path,required=True);p.add_argument('--lock',type=Path,required=True);p.add_argument('--output',type=Path,required=True);p.add_argument('--oxidex',type=Path);p.add_argument('--source',type=Path,default=compiler.SNAPSHOT);a=p.parse_args()
    if a.output.exists():p.error('evidence output must be new')
    a.output.mkdir(parents=True)
    native=a.lib.parent/'exiftool'
    def hashes():
        paths=[a.perl,native,a.lib/'Image/ExifTool.pm',a.lib/'Image/ExifTool/QuickTime.pm',a.lib/'Image/ExifTool/Charset.pm',a.lib/'Image/ExifTool/Charset/MacRoman.pm',a.lib/'Image/ExifTool/XMP.pm',a.source,Path(__file__),Path(compiler.__file__)]
        if a.oxidex:paths.append(a.oxidex)
        return {str(x):hashlib.sha256(x.read_bytes()).hexdigest() for x in paths}
    status=a.output/'status.json';status.write_text(json.dumps({'stage':'queued'}))
    with a.lock.open('a') as lock:
        fcntl.flock(lock,fcntl.LOCK_EX)
        before=hashes()
        runtime_before=runtime_inputs.runtime_input_manifest(compiler.ROOT) if a.oxidex else None
        source=json.loads(a.source.read_text());compiled=compiler.compile_document(source)
        if not compiled['protocol']['eligible']:p.error('source protocol refused')
        proc=source['modules']['QuickTime']['tables']['UserData']['meta']['PROCESS_PROC']
        if before[str(a.lib/'Image/ExifTool/QuickTime.pm')]!=proc['source_sha256']:raise ValueError('native processor source differs')
        for fact in source['quicktime_userdata_reader_protocol']['dependencies'].values():
            if before[str(a.lib/fact['source_file'])]!=fact['source_sha256']:raise ValueError('native helper source differs')
        fact=source['quicktime_userdata_reader_protocol']['charset_map']
        if before[str(a.lib/fact['source_file'])]!=fact['source_sha256']:raise ValueError('native charset differs')
        command=[str(a.perl),str(native),'-config','']
        version=subprocess.check_output(command+['-ver'],text=True).strip()
        if version!=source['exiftool_version']:raise ValueError('native version differs')
        import sys
        sys.path.insert(0,str(compiler.ROOT/'scripts'));import instrument
        state=instrument.git_state(compiler.ROOT)
        instrument.print_header(tool='verify_quicktime_userdata_reader.py', git=state,
            extra=['pinned Perl: '+str(a.perl), 'pinned native: '+str(native),
                   'native '+version+'; explicit format fixture capability checked per target',
                   'Rust public route: '+str(bool(a.oxidex))])
        if a.oxidex:
            instrument.refuse_if_dirty(state,'verify_quicktime_userdata_reader.py')
            note=instrument.staleness_note(instrument.resolve_binary(str(a.oxidex)),state)
            if note:raise ValueError('binary staleness: '+str(note))
        rows=cases(compiled['specs']);failures=[]
        for i,row in enumerate(rows):
            status.write_text(json.dumps({'stage':'native','completed':i,'total':len(rows)}))
            path=a.output/(row['case']+'.mov');data=fixture(row);path.write_bytes(data)
            row['fixture_sha256']=hashlib.sha256(data).hexdigest();row['expected']={}
            for mode,flags in [('print',[]),('raw',['-n'])]:
                cmd=command+['-j','-a','-G1','-s']+flags+[str(path)]
                run=subprocess.run(cmd,capture_output=True,check=True)
                prefix=a.output/(row['case']+'.'+mode)
                prefix.with_suffix(prefix.suffix+'.stdout.json').write_bytes(run.stdout)
                prefix.with_suffix(prefix.suffix+'.stderr.log').write_bytes(run.stderr)
                doc=json.loads(run.stdout)[0]
                if row['target'] not in doc:raise ValueError('native target absent: '+row['case'])
                val=value_text(doc[row['target']]);row['expected'][mode]={'utf8_sha256':hashlib.sha256(val.encode()).hexdigest(),'utf8_length':len(val.encode()),'transcript_sha256':hashlib.sha256(run.stdout).hexdigest()}
                if a.oxidex:
                    actual=subprocess.run([str(a.oxidex),'-j','-a','-G1']+(['--no-print-conv'] if mode=='raw' else [])+[str(path)],capture_output=True,check=True)
                    prefix.with_suffix(prefix.suffix+'.oxidex.json').write_bytes(actual.stdout)
                    got=json.loads(actual.stdout)[0]
                    if row['target'] not in got or value_text(got[row['target']])!=val:failures.append([row['case'],mode])
        after=hashes()
        if after!=before:raise ValueError('input changed during probe')
        if a.oxidex and runtime_inputs.runtime_input_manifest(compiler.ROOT)!=runtime_before:
            raise ValueError('runtime source changed during probe')
        result={'schema':'quicktime_userdata_native_fixtures_v1','source_hashes':before,'exiftool_version':version,'rust_public_route_checked':bool(a.oxidex),'runtime_input_manifest_sha256':runtime_before,'cases':rows,'failures':failures}
        (a.output/'fixtures.json').write_text(json.dumps(result,sort_keys=True,indent=2)+'\n')
        status.write_text(json.dumps({'stage':'done','cases':len(rows),'native_operations':len(rows)*2,'rust_public_route_checked':bool(a.oxidex),'failures':failures},indent=2))
        if failures:raise SystemExit(1)
if __name__=='__main__':main()
