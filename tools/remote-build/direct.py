#!/usr/bin/env python3
"""Cargo external command adapter for a configured OxiDex checkout."""
import argparse
import hashlib
import os
import subprocess
import sys
from datetime import datetime, timezone
from pathlib import Path
sys.path.insert(0,str(Path(__file__).resolve().parents[2]/'scripts'))
from ops_paths import ops_root
from lib.config import configured_remote_env
from lib.remote_build import main

parser=argparse.ArgumentParser(description='Build on a configured remote VM. Instance, zone, project and checkout are resolved from environment/Cargo configuration.')
task=parser.add_mutually_exclusive_group()
task.add_argument('--profile',choices=['debug','release','test'])
task.add_argument('--just-recipe')
parser.add_argument('--just-arg',action='append',default=[])
parser.add_argument('--approved-linux-perl-envelope',type=Path)
parser.add_argument('--evidence-dir',type=Path)
parser.add_argument('--artifact-dir',type=Path)
options=parser.parse_args()
if not options.profile and not options.just_recipe:
    options.profile='release'


source=Path(subprocess.check_output(['git','rev-parse','--show-toplevel'],text=True).strip())
if options.just_arg and not options.just_recipe:
    parser.error('--just-arg requires --just-recipe')
if bool(options.approved_linux_perl_envelope) != (options.just_recipe == 'prove-linux-perl-component'):
    parser.error('approved Linux Perl envelope belongs only to component proof')
if options.approved_linux_perl_envelope and options.just_arg:
    parser.error('component proof does not take public Just arguments')
os.environ.update(configured_remote_env(source))
def setting(name):
    value=os.environ.get(name)
    if value:
        return value
    if name=='OXIDEX_REMOTE_WORKTREE':
        return 'oxidex-'+hashlib.sha256(str(source).encode()).hexdigest()[:12]
    if name=='OXIDEX_REMOTE_PROJECT':
        value=subprocess.check_output(['gcloud','config','get-value','project'],text=True).strip()
        if value and value!='(unset)':
            return value
    raise RuntimeError('Configure '+name+' in the environment or .cargo/config.toml')
stamp=datetime.now(timezone.utc).strftime('%Y%m%dT%H%M%S%fZ')
args=['--source',str(source),'--instance',setting('OXIDEX_REMOTE_INSTANCE'),
      '--zone',setting('OXIDEX_REMOTE_ZONE'),'--project',setting('OXIDEX_REMOTE_PROJECT'),
      '--worktree-id',setting('OXIDEX_REMOTE_WORKTREE'),
      '--evidence-dir',str(ops_root()/'evidence'/('cargo-remote-'+stamp))]
if os.environ.get('OXIDEX_REMOTE_INSTANCE_ID'):
    args += ['--instance-id', os.environ['OXIDEX_REMOTE_INSTANCE_ID']]
args += (['--profile',options.profile] if options.profile else
         ['--just-recipe',options.just_recipe]
         +['--just-arg='+value for value in options.just_arg]
         +(['--approved-linux-perl-envelope='+str(options.approved_linux_perl_envelope)]
           if options.approved_linux_perl_envelope else []))
if options.evidence_dir:
    args += ['--evidence-dir',str(options.evidence_dir)]
if options.artifact_dir:
    args += ['--artifact-dir',str(options.artifact_dir)]
raise SystemExit(main(args))
