#!/usr/bin/env python3
"""Choose a lightly used worker, then delegate the build over SSH."""
import argparse
import os
import re
import subprocess
import sys
from datetime import datetime, timezone
from pathlib import Path
sys.path.insert(0,str(Path(__file__).resolve().parents[2]/'scripts'))
from ops_paths import ops_root
from lib.config import configured_remote_env
from lib.worker_selection import select_worker
from lib.ssh_transport import identity as identity_for_recipe


def main():
    parser=argparse.ArgumentParser(description=__doc__)
    task=parser.add_mutually_exclusive_group(required=True)
    task.add_argument('--profile', choices=['debug','release','test'])
    task.add_argument('--just-recipe')
    parser.add_argument('--just-arg',action='append',default=[])
    parser.add_argument('--approved-linux-perl-envelope',type=Path)
    parser.add_argument("--max-attempts",type=int,default=3)
    args=parser.parse_args()
    if args.max_attempts < 1:
        parser.error("--max-attempts must be positive")
    if args.just_arg and not args.just_recipe:
        parser.error('--just-arg requires --just-recipe')
    if args.just_recipe and not re.fullmatch(r'[a-z][a-z0-9_-]{0,63}',args.just_recipe):
        parser.error('invalid Just recipe name')
    if bool(args.approved_linux_perl_envelope) != (args.just_recipe == 'prove-linux-perl-component'):
        parser.error('approved Linux Perl envelope belongs only to component proof')
    if args.approved_linux_perl_envelope and args.just_arg:
        parser.error('component proof does not take public Just arguments')
    if any(not value or len(value)>4096 or any(ch in value for ch in '\x00\n\r') for value in args.just_arg):
        parser.error('invalid Just argument')
    source=Path(subprocess.check_output(['git','rev-parse','--show-toplevel'],text=True).strip())
    configured=configured_remote_env(source)
    os.environ.update(configured)  # only named Cargo keys; selector and child share identity
    child_env=dict(os.environ)
    if args.just_recipe and (not identity_for_recipe(child_env) or not configured.get('OXIDEX_REMOTE_SSH_KNOWN_HOSTS')):
        raise ValueError('Generic Just recipe requires explicit uploader user/key and trusted known-hosts file')
    project=configured.get('OXIDEX_REMOTE_PROJECT')
    if not project:
        project=subprocess.check_output(['gcloud','config','get-value','project'],text=True).strip()
    if not project or project=='(unset)':
        raise RuntimeError('Set OXIDEX_REMOTE_PROJECT or configure a gcloud project')
    stamp=datetime.now(timezone.utc).strftime('%Y%m%dT%H%M%S%fZ')
    evidence=ops_root()/'evidence'/('auto-remote-'+stamp)
    task_args=(['--profile',args.profile] if args.profile else ['--just-recipe',args.just_recipe]
               +['--just-arg='+value for value in args.just_arg]
               +(['--approved-linux-perl-envelope='+str(args.approved_linux_perl_envelope)]
                 if args.approved_linux_perl_envelope else []))
    if configured.get('OXIDEX_REMOTE_INSTANCE'):
        # Explicit bootstrap route: actual provider ID and authenticated launcher
        # are checked by direct.py; host admission still enforces resources.
        from lib.ssh_transport import identity
        if not identity(child_env) or not configured.get('OXIDEX_REMOTE_INSTANCE_ID') or not configured.get('OXIDEX_REMOTE_ZONE') or not configured.get('OXIDEX_REMOTE_SSH_KNOWN_HOSTS'):
            raise ValueError('Explicit builder requires instance ID, zone and SSH user/key')
        return subprocess.call([sys.executable, str(Path(__file__).with_name('direct.py')),
            *task_args, '--evidence-dir', str(evidence)],env=child_env)
    return run_attempts(project, source, task_args, args.profile, evidence,
                        1 if args.just_recipe else args.max_attempts, child_env)


def run_attempts(project, source, task_args, profile, evidence, max_attempts, configured_env):
    import json
    excluded=set()
    for attempt in range(1, max_attempts+1):
        # Inventory and Monitoring are read again on every attempt.
        vm,sample=select_worker(project, excluded_ids=excluded)
        env=dict(configured_env,OXIDEX_REMOTE_INSTANCE=vm.name,OXIDEX_REMOTE_ZONE=vm.zone,
                 OXIDEX_REMOTE_PROJECT=project, OXIDEX_REMOTE_INSTANCE_ID=str(vm.instance_id))
        folder=evidence/str(attempt)
        folder.mkdir(parents=True)
        (folder/'selection.json').write_text(json.dumps({'instance':vm.name,'instance_id':vm.instance_id,
            'zone':vm.zone,'cpu':sample[0],'memory':sample[1]},indent=2))
        command=[sys.executable,str(Path(__file__).with_name('direct.py')),*task_args,
                 '--evidence-dir',str(folder)]
        if profile:
            command += ['--artifact-dir',str(source/'target'/'remote-linux'/profile)]
        result=subprocess.call(command,env=env)
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
