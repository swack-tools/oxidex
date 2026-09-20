#!/bin/bash
# Authenticated generated-route census. Invoke through the exclusive lock.
set -euo pipefail
usage() { echo "usage: census.sh --repository REPO --target-dir DIR --output DIR --corpus DIR --perl PERL --exiftool-dir DIR --tokens LIST" >&2; }
repository= target_dir= output= corpus= perl= exiftool_dir= tokens=
while (($#)); do
  case "$1" in
    --repository|--target-dir|--output|--corpus|--perl|--exiftool-dir|--tokens) (($# >= 2)) || { usage; exit 2; }; key=${1#--}; key=${key//-/_}; printf -v "$key" '%s' "$2"; shift 2 ;;
    -h|--help) usage; exit 0 ;;
    *) echo "unknown argument: $1" >&2; usage; exit 2 ;;
  esac
done
for required in repository target_dir output corpus perl exiftool_dir tokens; do [[ -n ${!required} ]] || { echo "missing --${required//_/-}" >&2; usage; exit 2; }; done
IFS=, read -r -a token_list <<< "$tokens"
valid=(engine legacy-l1 legacy-l2 producers serial keyed)
for token in "${token_list[@]}"; do
  [[ -n $token ]] || { echo "empty genshare token" >&2; exit 2; }
  [[ $token != conv ]] || { echo "unsafe genshare token: conv" >&2; exit 2; }
  found=0; for allowed in "${valid[@]}"; do [[ $token == "$allowed" ]] && found=1; done
  ((found)) || { echo "unknown genshare token: $token" >&2; exit 2; }
done
unset PERL5LIB PERLLIB PERL5OPT OXIDEX_GENSHARE_SILENCE
[[ -d $repository && -d $corpus && -x $perl && -d $exiftool_dir ]] || { echo "invalid census path" >&2; exit 2; }
[[ $(git -C "$repository" status --porcelain) == "" ]] || { echo "repository must be clean" >&2; exit 2; }
[[ $($perl -I"$exiftool_dir/lib" "$exiftool_dir/exiftool" -ver) == 13.59 ]] || { echo "oracle version refusal" >&2; exit 2; }
[[ $($perl -I"$exiftool_dir/lib" "$exiftool_dir/exiftool" -s3 -FileType "$exiftool_dir/t/images/OOXML.docx") == DOCX ]] || { echo "oracle DOCX refusal" >&2; exit 2; }
manifest="$repository/tools/exiftool-tables/genshare/testdata/bounded-corpus.txt"
entries=()
while IFS= read -r entry; do [[ -n $entry ]] && entries+=("$entry"); done < "$manifest"
((${#entries[@]} == 3)) || { echo "bounded corpus must contain exactly three files" >&2; exit 2; }
files=(); for entry in "${entries[@]}"; do [[ -f "$corpus/$entry" ]] || { echo "missing corpus file: $entry" >&2; exit 2; }; files+=("$corpus/$entry"); done
mkdir -p "$output" "$target_dir"; export CARGO_TARGET_DIR="$target_dir"
(cd "$repository" && cargo build --release --bin oxidex)
binary="$target_dir/release/oxidex"; [[ -x $binary ]] || { echo "build did not produce $binary" >&2; exit 2; }
run_conformance() {
  local label=$1 silence=${2-} rc
  set +e
  (cd "$repository" && OXIDEX_GENSHARE_SILENCE="$silence" python3 tools/exiftool-tables/conformance.py "${files[@]}" --exiftool-dir "$exiftool_dir" --oxidex "$binary" --min-files 3 --min-tags 1 --json-out "$output/$label.json") >"$output/$label.stdout" 2>"$output/$label.stderr"
  rc=$?; set -e; printf '%s' "$rc" > "$output/$label.returncode"; ((rc == 0)) || return "$rc"
}
run_conformance control ''
for token in "${token_list[@]}"; do run_conformance "probe-$token" "$token"; done
python3 - "$binary" "$output/inertness.json" "${files[@]}" <<'PY'
import json, os, subprocess, sys
binary, out, *files = sys.argv[1:]; env = {k:v for k,v in os.environ.items() if k != 'OXIDEX_GENSHARE_SILENCE'}
drop = {'File:FileAccessDate', 'File:FileInodeChangeDate', 'FileAccessDate', 'FileInodeChangeDate'}
def read(path):
 p=subprocess.run([binary,'-j','-G1','-a',path],capture_output=True,env=env,timeout=120)
 try: doc=json.loads(p.stdout or b'[]')
 except ValueError: return p.returncode,p.stdout.hex()
 for obj in doc:
  for key in list(obj):
   if key in drop or key.endswith(':FileAccessDate') or key.endswith(':FileInodeChangeDate'): del obj[key]
 return p.returncode,doc
diffs=[path for path in files if read(path)!=read(path)]
json.dump({'equal':not diffs,'files':len(files),'differences':len(diffs),'paths':diffs},open(out,'w'),indent=2)
raise SystemExit(0 if not diffs else 1)
PY
python3 - "$repository" "$perl" "$exiftool_dir" "$manifest" "$output" "$binary" "$tokens" <<'PY'
import hashlib,json,pathlib,subprocess,sys
repo,perl,et,manifest,out,binary=map(pathlib.Path,sys.argv[1:7]); tokens=sys.argv[7].split(',')
def sha(path): return hashlib.sha256(path.read_bytes()).hexdigest()
def data(path): return json.loads(path.read_text())
def matched(path): return sum(row.get('matched',0) for row in data(path)['per_format'].values())
def proc(label): return {'returncode':int((out/f'{label}.returncode').read_text()),'binary_sha256':sha(binary),'stdout_sha256':sha(out/f'{label}.stdout'),'stderr_sha256':sha(out/f'{label}.stderr'),'output_sha256':sha(out/f'{label}.json')}
control=proc('control'); base=matched(out/'control.json'); probes={token:proc(f'probe-{token}') for token in tokens}
def occurrence_delta(control_path, probe_path):
 control_doc, probe_doc = data(control_path), data(probe_path)
 total = 0
 for path, control_file in control_doc.get('per_file', {}).items():
  probe_file = probe_doc.get('per_file', {}).get(path)
  if probe_file is None: continue
  def missing(file):
   return sum(len(values) if isinstance(values, list) else 1 for values in file.get('missing', {}).values())
  total += missing(probe_file) - missing(control_file)
  total += len(probe_file.get('value_diff', [])) - len(control_file.get('value_diff', []))
 lost = matched(control_path) - matched(probe_path)
 renames = sum(row.get('renames', 0) for row in data(probe_path)['per_format'].values()) - sum(row.get('renames', 0) for row in data(control_path)['per_format'].values())
 return {'matched_lost': lost, 'occurrence_lost': total, 'rename_delta': renames, 'residual': lost - total - renames}
source={'commit':subprocess.check_output(['git','-C',str(repo),'rev-parse','HEAD'],text=True).strip(),'tree':subprocess.check_output(['git','-C',str(repo),'rev-parse','HEAD^{tree}'],text=True).strip(),'dirty':False}
receipt={'schema':'genshare-receipt/v2','source':source,'oracle':{'version':'13.59','perl_sha256':sha(perl),'exiftool_sha256':sha(et/'exiftool'),'docx_filetype':'DOCX'},'corpus':{'manifest_sha256':sha(manifest),'files':3},'token_set':tokens,'control':control,'probes':probes,'inertness':data(out/'inertness.json'),'per_occurrence_deltas':{token:occurrence_delta(out/'control.json',out/f'probe-{token}.json') for token in tokens}}
(all(delta['residual'] == 0 for delta in receipt['per_occurrence_deltas'].values())) or (_ for _ in ()).throw(SystemExit('attribution reconciliation failure'))
(out/'receipt.json').write_text(json.dumps(receipt,indent=2,sort_keys=True)+'\n')
PY
python3 "$repository/tools/exiftool-tables/genshare/attribute.py" --validate-receipt "$output/receipt.json"
