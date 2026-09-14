#!/usr/bin/env python3
"""Evidence instrument for direct QuickTime Keys `meta/keys/ilst` behavior."""
from __future__ import annotations
import argparse,hashlib,json,subprocess,tempfile
from pathlib import Path
import quicktime_keys_specs as specs
import quicktime_baseline as baseline

SCHEMA='oxidex_quicktime_generated_keys_read_evidence_v1'
def atom(name,payload): return (len(payload)+8).to_bytes(4,'big')+name+payload
def fixture(name,namespace=b'mdta',key=b'com.apple.quicktime.artist',index=1,flags=1,value=b'Ada',count=1):
 keys=b'\0'*4+count.to_bytes(4,'big')+(8+len(key)).to_bytes(4,'big')+namespace+key
 data=atom(b'data',flags.to_bytes(4,'big')+b'\0'*4+value)
 ilst=atom(index.to_bytes(4,'big'),data)
 hdlr=atom(b'hdlr',b'\0'*8+b'mdta'+b'\0'*12+b'\0')
 meta=atom(b'meta',b'\0'*4+hdlr+atom(b'keys',keys)+atom(b'ilst',ilst))
 return atom(b'ftyp',b'isom\0\0\0\0isom')+atom(b'moov',atom(b'udta',meta))
def cases():
 return {
  'prefix':fixture('prefix'), 'full-retry':fixture('full',key=b'com.android.model',value=b'Pixel'),
  'ordinal':fixture('ordinal',index=2), 'count-ignored':fixture('count',count=0),
  'utf8':fixture('utf8',value='Ada'.encode()), 'numeric-enum':fixture('enum',key=b'player.movie.audio.mute',flags=21,value=b'\1'),
  'unknown':fixture('unknown',key=b'not.source.defined',value=b'x'),
  'refused-gps':fixture('gps',key=b'com.apple.quicktime.location.ISO6709',value=b'+1.0-2.0/'),
  'refused-date':fixture('date',key=b'com.apple.quicktime.creationdate',value=b'2020-01-01T00:00:00Z')}
def authenticated(source,ledger,rust):
 doc=json.loads(source.read_text()); expected=specs.compile_document(doc)
 if expected!=json.loads(ledger.read_text()) or specs.render_rust(expected)!=rust.read_text(): raise ValueError('Keys source artifacts do not replay')
 return {k:hashlib.sha256(p.read_bytes()).hexdigest() for k,p in {'source_sha256':source,'ledger_sha256':ledger,'rust_sha256':rust}.items()}
def validate_report(report, ledger):
 by_name={x['name']:x['source_identity'] for x in ledger['specs']}
 credited=[]
 for row in report['observations']:
  for side in ('native_json','oxidex_json'):
   if hashlib.sha256(row[side].encode()).hexdigest()!=row[side+'_sha256']: raise ValueError('transcript hash differs')
  native=baseline.projection(json.loads(row['native_json'])); actual=baseline.projection(json.loads(row['oxidex_json']))
  if native!=row['expected'] or actual!=row['actual'] or row['matched'] != (native==actual): raise ValueError('transcript projection claim differs')
  if row['matched']:
   for key in actual:
    if key.startswith('Keys:') and key.removeprefix('Keys:') in by_name: credited.append({'fixture':row['fixture'],'source_identity':by_name[key.removeprefix('Keys:')],'group1':'Keys','tag_name':key.removeprefix('Keys:')})
 return credited

def compare(tree,out,source,ledger,rust):
 inp=authenticated(source,ledger,rust); ledger_doc=json.loads(ledger.read_text()); root=baseline.ROOT; state=baseline.instrument.git_state(root)
 if state.dirty: raise ValueError('clean checkout required')
 oracle=baseline.exiftool_oracle.resolve_tree(tree.resolve()); binary=baseline.instrument.resolve_binary(root/'target/debug/oxidex')
 rows=[]; manifest={}
 out.mkdir();
 for name,data in cases().items():
  path=out/(name+'.mp4');path.write_bytes(data); manifest[name]=hashlib.sha256(data).hexdigest()
  native=subprocess.run(oracle.command(['-config','', '-j','-a','-G1','-s',str(path)]),text=True,capture_output=True,check=True)
  ox=subprocess.run([str(binary.path),'-j','-a','-G1',str(path)],text=True,capture_output=True,check=True)
  expected=baseline.projection(json.loads(native.stdout)); actual=baseline.projection(json.loads(ox.stdout))
  rows.append({'fixture':name,'fixture_sha256':manifest[name],'native_json':native.stdout,'native_json_sha256':hashlib.sha256(native.stdout.encode()).hexdigest(),'oxidex_json':ox.stdout,'oxidex_json_sha256':hashlib.sha256(ox.stdout.encode()).hexdigest(),'expected':expected,'actual':actual,'matched':expected==actual,'native_emitted':bool(expected),'oxidex_emitted':bool(actual)})
 report={'schema':SCHEMA,'inputs':inp,'source_commit':state.commit,'binary_sha256':hashlib.sha256(Path(binary.path).read_bytes()).hexdigest(),'producer':{'runtime_input_manifest_sha256':__import__('runtime_evidence_inputs').runtime_input_manifest(root),'fixture_manifest_sha256':hashlib.sha256(json.dumps(manifest,sort_keys=True).encode()).hexdigest()},'observations':rows,'matches':[r for r in rows if r['matched'] and r['native_emitted'] and r['oxidex_emitted']],'absence_or_refusals':[r for r in rows if not (r['matched'] and r['native_emitted'] and r['oxidex_emitted'])],'scope':'direct Keys-table meta/keys/ilst fixtures only; unknown and unsupported source rows are recorded separately from matches'}
 report['matched_identities']=validate_report(report,ledger_doc)
 (out/'comparison.json').write_text(json.dumps(report,indent=2)+'\n');return report
def main():
 p=argparse.ArgumentParser();p.add_argument('--exiftool-dir',type=Path,required=True);p.add_argument('--out',type=Path,required=True);p.add_argument('--source',type=Path,required=True);p.add_argument('--ledger',type=Path,required=True);p.add_argument('--rust',type=Path,required=True);a=p.parse_args();r=compare(a.exiftool_dir,a.out,a.source,a.ledger,a.rust);print(f"Keys emitted matches: {len(r['matches'])}/{len(r['observations'])}")
if __name__=='__main__':main()
