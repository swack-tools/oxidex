#!/usr/bin/env python3
"""Choose a lightly used worker, then delegate the build over SSH."""
import argparse
import os
import subprocess
import sys
import tomllib
from datetime import datetime, timezone
from pathlib import Path
from lib.worker_selection import select_worker


def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--profile', choices=['debug','release'],required=True)
    parser.add_argument("--max-attempts",type=int,default=3)
    args=parser.parse_args()
    if args.max_attempts < 1:
        parser.error("--max-attempts must be positive")
    source=Path(subprocess.check_output(['git','rev-parse','--show-toplevel'],text=True).strip())
    settings=tomllib.loads((source/'.cargo/config.toml').read_text()).get('env',{})
    project=os.environ.get('OXIDEX_REMOTE_PROJECT') or settings.get('OXIDEX_REMOTE_PROJECT')
    if not project:
        project=subprocess.check_output(['gcloud','config','get-value','project'],text=True).strip()
    if not project or project=='(unset)':
        raise RuntimeError('Set OXIDEX_REMOTE_PROJECT or configure a gcloud project')
    stamp=datetime.now(timezone.utc).strftime('%Y%m%dT%H%M%S%fZ')
    evidence=Path.home()/'oxidex-ops'/'evidence'/('auto-remote-'+stamp)
    return run_attempts(project, source, args.profile, evidence, args.max_attempts)


def run_attempts(project, source, profile, evidence, max_attempts):
    import json
    excluded=set()
    for attempt in range(1, max_attempts+1):
        # Inventory and Monitoring are read again on every attempt.
        vm,sample=select_worker(project, excluded_ids=excluded)
        env=dict(os.environ,OXIDEX_REMOTE_INSTANCE=vm.name,OXIDEX_REMOTE_ZONE=vm.zone,
                 OXIDEX_REMOTE_PROJECT=project)
        folder=evidence/str(attempt)
        folder.mkdir(parents=True)
        (folder/'selection.json').write_text(json.dumps({'instance':vm.name,'instance_id':vm.instance_id,
            'zone':vm.zone,'cpu':sample[0],'memory':sample[1]},indent=2))
        result=subprocess.call([sys.executable,str(Path(__file__).with_name('direct.py')),'--profile',profile,
            '--evidence-dir',str(folder),'--artifact-dir',str(source/'target'/'remote-linux'/profile)],env=env)
        if result==0:
            return 0
        receipt=folder/'remote-build.json'
        if not receipt.exists() or not json.loads(receipt.read_text()).get('retryable',False):
            return result
        excluded.add(vm.instance_id)
        print(f'Worker {vm.name} interrupted; refreshing CPU/RAM placement (attempt {attempt}/{max_attempts})',flush=True)
    return result



if __name__ == '__main__':
    raise SystemExit(main())
