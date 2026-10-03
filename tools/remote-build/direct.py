#!/usr/bin/env python3
"""Cargo external command adapter for a configured OxiDex checkout."""
import hashlib
import os
import subprocess
import sys
import tomllib
from datetime import datetime, timezone
from pathlib import Path
sys.path.insert(0,str(Path(__file__).resolve().parents[2]/'scripts'))
from ops_paths import ops_root
from lib.remote_build import main

if '--help' in sys.argv[1:] or '-h' in sys.argv[1:]:
    raise SystemExit(main(sys.argv[1:]))

source=Path(subprocess.check_output(['git','rev-parse','--show-toplevel'],text=True).strip())
settings=tomllib.loads((source/'.cargo/config.toml').read_text()).get('env',{})
def setting(name):
    value=os.environ.get(name) or settings.get(name)
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
raise SystemExit(main(args+sys.argv[1:]))
