"""Snapshot a local checkout, transfer it over authenticated SSH, and time builds."""
import argparse
import hashlib
import json
import shlex
import subprocess
import tarfile
import time
from pathlib import Path


def retryable_exit(code):
    return code in (75, 137, 143, 255)


def download_artifact(instance, zone, project, binary, artifact, digest):
    import tempfile
    import os
    fd,name=tempfile.mkstemp(prefix='.oxidex-download-',dir=artifact.parent)
    os.close(fd)
    temporary=Path(name)
    try:
        subprocess.run(['gcloud','compute','scp',instance+':'+binary,str(temporary),
            '--zone='+zone,'--project='+project,'--quiet','--scp-flag=-C'],check=True)
        if hashlib.sha256(temporary.read_bytes()).hexdigest()!=digest:
            raise RuntimeError('Downloaded binary checksum mismatch')
        temporary.chmod(0o755)
        temporary.replace(artifact)
    finally:
        temporary.unlink(missing_ok=True)


def make_snapshot(source: Path, archive: Path) -> dict:
    names = subprocess.check_output(['git','-C',str(source),'ls-files','-z']).decode().split('\0')
    files=[]
    with tarfile.open(archive,'w:gz',compresslevel=3) as tar:
        for name in names:
            parts=Path(name).parts
            if not name or any(p in {'.git','.codex','.claude','.agents','target','node_modules','.venv'} for p in parts):
                continue
            path=source/name
            if path.name.startswith('.env') or path.suffix in {'.pem','.key'} or not path.is_file() or path.is_symlink():
                continue
            files.append({'path':name,'sha256':hashlib.sha256(path.read_bytes()).hexdigest(),'bytes':path.stat().st_size})
            tar.add(path,arcname=name,recursive=False)
    return {'files':files,'file_count':len(files),'archive_bytes':archive.stat().st_size,
            'archive_sha256':hashlib.sha256(archive.read_bytes()).hexdigest()}


def main(argv=None):
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--source',type=Path,required=True)
    parser.add_argument('--instance',required=True)
    parser.add_argument('--zone',required=True)
    parser.add_argument('--project',default='homelab-424523')
    parser.add_argument('--worktree-id',required=True)
    parser.add_argument('--evidence-dir',type=Path,required=True)
    parser.add_argument('--profile',choices=['debug','release'],default='release')
    parser.add_argument('--artifact-dir',type=Path)
    args=parser.parse_args(argv)
    import re
    if not re.fullmatch(r'[a-zA-Z0-9][a-zA-Z0-9_-]{0,63}',args.worktree_id):
        raise ValueError('Invalid worktree identifier')
    source=args.source.expanduser().resolve();evidence=args.evidence_dir.expanduser().resolve()
    evidence.mkdir(parents=True,exist_ok=True)
    receipt={'profile':args.profile,'source':str(source),'instance':args.instance,'zone':args.zone,'project':args.project}
    def save():
        (evidence/'remote-build.json').write_text(json.dumps(receipt,indent=2))
    def ssh(command):
        return ['gcloud','compute','ssh',args.instance,'--zone='+args.zone,'--project='+args.project,
                '--quiet','--command='+command]
    try:
        start=time.monotonic();archive=evidence/'remote-source.tar.gz'
        receipt['snapshot']=make_snapshot(source,archive)
        receipt['packaging_seconds']=time.monotonic()-start
        receipt['source_commit']=subprocess.check_output(['git','-C',str(source),'rev-parse','HEAD'],text=True).strip()
        receipt['source_status']=subprocess.check_output(['git','-C',str(source),'status','--porcelain'],text=True)
        save()
        project=shlex.quote(args.worktree_id)
        subprocess.run(ssh(f'sudo /usr/local/bin/oxidex-remote-build {project} prepare'),check=True)
        start=time.monotonic()
        subprocess.run(['gcloud','compute','scp',str(archive),args.instance+':~/oxidex-remote-source-'+args.worktree_id+'.tar.gz',
                        '--zone='+args.zone,'--project='+args.project,'--quiet'],check=True)
        digest=receipt['snapshot']['archive_sha256']
        destination='/mnt/runner-data/remote-build/sources/'+args.worktree_id
        upload='oxidex-remote-source-'+args.worktree_id+'.tar.gz'
        command=(f"echo '{digest}  {upload}' | sha256sum -c - "
                 f"&& tar -xzf {upload} -C {shlex.quote(destination)}")
        subprocess.run(ssh(command),check=True)
        receipt['sync_seconds']=time.monotonic()-start;save()
        # Dependencies are fetched separately; compile time excludes downloads.
        stages=[('fetch','cargo fetch --locked --target x86_64-unknown-linux-gnu')]
        if args.profile=='release':
            stages.append(('header','just cbindgen-check'))
        stages.append(('compile','cargo build '+('--release ' if args.profile=='release' else '')+'--locked --bin oxidex'))
        for stage,command in stages:
            start=time.monotonic()
            remote=f'sudo /usr/local/bin/oxidex-remote-build {project} {command}'
            with (evidence/f'{stage}.log').open('w') as log:
                result=subprocess.run(ssh(remote),stdout=log,stderr=subprocess.STDOUT)
            receipt[stage+'_seconds']=time.monotonic()-start
            receipt[stage+'_exit_code']=result.returncode
            receipt['retryable']=retryable_exit(result.returncode);save()
            print(stage,receipt[stage+'_seconds'],'seconds; exit',result.returncode,flush=True)
            if result.returncode:
                raise RuntimeError(f'{stage} failed; see {evidence / (stage+".log")}')
        verify=f'sudo /usr/local/bin/oxidex-remote-build {project} cargo --version'
        subprocess.run(ssh(verify),check=True)
        binary=f'/mnt/runner-data/remote-build/targets/{args.worktree_id}/{args.profile}/oxidex'
        verification=subprocess.check_output(ssh(shlex.quote(binary)+' --version && sha256sum '+shlex.quote(binary)),text=True)
        print(verification,flush=True)
        digest=verification.splitlines()[-1].split()[0]
        receipt['binary_sha256']=digest
        if args.artifact_dir:
            output=args.artifact_dir.expanduser().resolve();output.mkdir(parents=True,exist_ok=True)
            artifact=output/'oxidex'
            download_artifact(args.instance,args.zone,args.project,binary,artifact,digest)
            receipt['artifact']=str(artifact)

        receipt['verified']=True;save()
    except Exception as exc:
        receipt['error']=str(exc)
        if isinstance(exc,subprocess.CalledProcessError):
            receipt['retryable']=retryable_exit(exc.returncode)
        save();raise
    print(json.dumps({k:v for k,v in receipt.items() if k!='snapshot'},indent=2))
    return 0
