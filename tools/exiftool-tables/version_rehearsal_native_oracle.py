#!/usr/bin/env python3
"""Readiness probes for one verified historical ExifTool source tree.

Not an OxiDex comparison: all native commands use an explicit Perl, materialized
lib and program, and mutations operate only on disposable fixture copies.
"""
from __future__ import annotations
import hashlib, os, re, shutil, subprocess, tempfile
from pathlib import Path
from typing import Any, Callable
import version_rehearsal as rehearsal
import version_rehearsal_catalog as catalog_stage
SCHEMA=2; KIND="oxidex_exiftool_version_rehearsal_native_capability"; TIMEOUT=20
TAG=re.compile(r"^[A-Za-z][A-Za-z0-9:]*$"); NAME=re.compile(r"^[A-Za-z0-9][A-Za-z0-9_.-]{0,79}$")
class Refused(ValueError): pass
def _sha(p:Path)->str:
 h=hashlib.sha256();
 with p.open('rb') as f:
  for b in iter(lambda:f.read(1048576),b''): h.update(b)
 return h.hexdigest()
def _regular(p:Path,label:str)->Path:
 if p.is_symlink(): raise Refused(f"{label} must not be a symbolic link")
 p=p.resolve()
 if not p.is_file(): raise Refused(f"{label} must be an existing regular file")
 return p
def _query(v:Any,label:str)->str:
 if not isinstance(v,str) or TAG.fullmatch(v) is None: raise Refused(f"{label} must be a tag name")
 return v
def _case(v:Any)->dict[str,Any]:
 if not isinstance(v,dict) or not isinstance(v.get('name'),str) or NAME.fullmatch(v['name']) is None: raise Refused('case name is unsafe')
 if not isinstance(v.get('fixture'),(str,Path)): raise Refused(f"{v['name']}: fixture is required")
 r=v.get('read'); w=v.get('write')
 if not isinstance(r,dict) or r.get('expectation') not in {'value','native_unsupported'}: raise Refused(f"{v['name']}: read expectation is unsupported")
 q=_query(r.get('query'),f"{v['name']} read query")
 if r['expectation']=='value' and not isinstance(r.get('value'),str): raise Refused(f"{v['name']}: read value required")
 if not isinstance(w,dict) or w.get('operation') not in {'set','delete'}: raise Refused(f"{v['name']}: write operation must be set or delete")
 tag=_query(w.get('tag'),f"{v['name']} write tag")
 if w['operation']=='set':
  if not isinstance(w.get('value'),str) or not isinstance(w.get('readback'),str): raise Refused(f"{v['name']}: set requires explicit string readback")
  if '\0' in w['value']: raise Refused(f"{v['name']}: NUL write requires a separate binary writer contract")
  if '\n' in w['value'] or '\n' in w['readback']: raise Refused(f"{v['name']}: newline scalar readback requires structured JSON contract")
 if w['operation']=='delete' and w.get('readback') is not None: raise Refused(f"{v['name']}: delete must require absent readback")
 expected=w.get('readback')
 if expected is not None and not isinstance(expected,str): raise Refused(f"{v['name']}: readback must be string or null")
 return {'name':v['name'],'fixture':Path(v['fixture']),'read':{'query':q,**r},'write':{'tag':tag,**w}}
def _env()->dict[str,str]:
 e=dict(os.environ)
 for k in ('PERL5LIB','PERLLIB','PERL5OPT','PERL_MM_OPT','PERL_MB_OPT','PERL_LOCAL_LIB_ROOT'): e.pop(k,None)
 return e
def _run(argv:list[str],run:Callable[...,subprocess.CompletedProcess[str]])->dict[str,Any]:
 try:
  x=run(argv,capture_output=True,text=True,errors='replace',timeout=TIMEOUT,env=_env())
  out,err=x.stdout or '',x.stderr or ''
  state='ok' if x.returncode==0 else 'exit_failed'
  return {'command':argv,'exit':x.returncode,'stdout':out,'stderr':err,'stdout_sha256':hashlib.sha256(out.encode()).hexdigest(),'stderr_sha256':hashlib.sha256(err.encode()).hexdigest(),'state':state}
 except subprocess.TimeoutExpired as e: return {'command':argv,'exit':None,'stdout':'','stderr':str(e),'state':'timeout'}
 except OSError as e: return {'command':argv,'exit':None,'stdout':'','stderr':str(e),'state':'spawn_failed'}
def _capability(perl:Path,run):
 rows=[]
 for m in ('Archive::Zip',): rows.append({'module':m,**_run([str(perl),f'-M{m}','-e','1'],run)})
 return {'required_modules':['Archive::Zip'],'modules':rows,'available':all(x['state']=='ok' for x in rows)}
def _read(prefix,tag,target,run): return _run([*prefix,'-s','-s','-s',f'-{tag}',str(target)],run)
def _matches(record,expect,value=None):
 if record['state']!='ok': return False
 got=record['stdout'].rstrip('\n')
 return (got=='' if expect=='native_unsupported' else got==value)
def probe_materialized_native(materialization,plan,catalog,capture,resolution,archive_cache,source_root,release,perl,cases,*,run=subprocess.run):
 catalog_stage.verify_source_materialization(materialization,plan,catalog,capture,resolution,archive_cache,source_root)
 row=next((x for x in materialization['selected_releases'] if x['release']==release),None)
 if row is None: raise Refused('native capability release is not materialized by this plan')
 source=(Path(source_root)/row['source_directory']).resolve(); pp=_regular(Path(perl),'Perl interpreter'); prog=_regular(source/'exiftool','materialized ExifTool program'); lib=(source/'lib').resolve()
 if lib.is_symlink() or not lib.is_dir(): raise Refused('materialized ExifTool lib directory is unavailable')
 parsed=[_case(x) for x in cases]
 if not parsed or len({x['name'] for x in parsed})!=len(parsed): raise Refused('at least one uniquely named capability case is required')
 prefix=[str(pp),f'-I{lib}',str(prog)]; version=_run([*prefix,'-ver'],run); cap=_capability(pp,run)
 identity={'release':release,'expected_version':release,'materialization_sha256':materialization['materialization_sha256'],'source_directory':row['source_directory'],'source_tree_sha256':row['tree']['tree_sha256'],'perl':{'path':str(pp),'sha256':_sha(pp)},'lib':{'path':str(lib)},'program':{'path':str(prog),'sha256':_sha(prog)}}
 ready=version['state']=='ok' and version['stdout'].strip()==release and cap['available']; records=[]
 if ready:
  with tempfile.TemporaryDirectory(prefix='oxidex-native-capability-') as tmp:
   for case in parsed:
    f=_regular(case['fixture'],f"{case['name']} fixture"); private=Path(tmp)/(hashlib.sha256(case['name'].encode()).hexdigest()+f.suffix); shutil.copyfile(f,private)
    rec={'name':case['name'],'fixture':{'sha256':_sha(f),'bytes':f.stat().st_size},'copy_sha256_before':_sha(private)}
    initial=_read(prefix,case['read']['query'],private,run); rec['read']=initial; read_ok=_matches(initial,case['read']['expectation'],case['read'].get('value'))
    op=case['write']; arg=f"-{op['tag']}=" if op['operation']=='delete' else f"-{op['tag']}={op['value']}"; write=_run([*prefix,'-overwrite_original',arg,str(private)],run); rec['write']=write
    readback=_read(prefix,op['tag'],private,run); rec['readback']=readback; write_ok=write['state']=='ok' and (_matches(readback,'value',op['readback']) if op.get('readback') is not None else _matches(readback,'native_unsupported'))
    rec['state']='ready' if read_ok and write_ok else 'failed'; rec['copy_sha256_after']=_sha(private); records.append(rec)
 state='ready' if ready and all(x['state']=='ready' for x in records) else 'failed'
 payload={'schema':SCHEMA,'kind':KIND,'identity':identity,'version':version,'perl_capability':cap,'cases':records,'state':state,'execution':{'native_read':'probed' if records else 'failed','native_write':'probed' if records else 'failed','conformance':'unrun','limit':'matching-native readiness only; not OxiDex/native conformance'}}
 return {**payload,'probe_sha256':catalog_stage.sha256_json(payload)}
def write_probe_report(path:Path,report:dict[str,Any])->None:
 if path.exists() or path.is_symlink(): raise Refused('native capability output already exists')
 payload={k:v for k,v in report.items() if k!='probe_sha256'}
 if report.get('probe_sha256')!=catalog_stage.sha256_json(payload): raise Refused('native capability report identity is malformed')
 rehearsal.atomic_json(path,report)
