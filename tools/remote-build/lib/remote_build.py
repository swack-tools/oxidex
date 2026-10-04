"""Snapshot a local checkout, transfer it over authenticated SSH, and time builds."""
import argparse
import hashlib
import json
import shlex
import re
import secrets
import subprocess
import sys
import tarfile
import io
import tomllib
import time
from pathlib import Path


SSH_KEEPALIVE = ('--ssh-flag=-oServerAliveInterval=15',
                 '--ssh-flag=-oServerAliveCountMax=3')
SCP_KEEPALIVE = ('--scp-flag=-oServerAliveInterval=15',
                 '--scp-flag=-oServerAliveCountMax=3')


def pinned_toolchain(source):
    pin = tomllib.loads((source/'rust-toolchain.toml').read_text())['toolchain']['channel']
    if not re.fullmatch(r'\d+\.\d+\.\d+', pin):
        raise RuntimeError('A numeric Rust toolchain pin is required')
    try:
        compiler = subprocess.check_output(
            ['rustup','which','--toolchain',pin,'rustc'], text=True).strip()
        cargo = subprocess.check_output(
            ['rustup','which','--toolchain',pin,'cargo'], text=True).strip()
        rustc_output = subprocess.check_output([compiler,'-vV'],text=True)
        cargo_output = subprocess.check_output([cargo,'-V'],text=True).strip()
    except (subprocess.CalledProcessError, FileNotFoundError) as exc:
        raise RuntimeError(f'Local rustup cannot resolve repository toolchain pin {pin}: {exc}') from exc
    commit = re.search(r'^commit-hash: ([0-9a-f]{40})$', rustc_output, re.M)
    release = re.search(r'^release: (\S+)$', rustc_output, re.M)
    if not commit or not release or release[1] != pin or not cargo_output.startswith('cargo '+pin+' '):
        raise RuntimeError('Local rustup cannot verify the repository toolchain pin')
    return {'channel':pin,'rustc_commit':commit[1],'cargo_version':cargo_output}


def verify_remote_toolchain(expected, ssh, project):
    rustc_output = subprocess.check_output(
        ssh(f'sudo /usr/local/bin/oxidex-remote-build {project} rustc -vV'), text=True)
    cargo_output = subprocess.check_output(
        ssh(f'sudo /usr/local/bin/oxidex-remote-build {project} cargo -V'), text=True)
    # The current entrypoint prints its selected pin before running the command.
    # Check both that declaration and the executable's own output.
    commits = re.findall(r'^commit-hash: ([0-9a-f]{40})$', rustc_output, re.M)
    releases = re.findall(r'^release: (\S+)$', rustc_output, re.M)
    cargo_versions = re.findall(r'^cargo \d+\.\d+\.\d+ [^\n]+$', cargo_output, re.M)
    if (len(commits) != 2 or set(commits) != {expected['rustc_commit']}
            or len(releases) != 2 or set(releases) != {expected['channel']}
            or len(cargo_versions) != 2 or set(cargo_versions) != {expected['cargo_version']}):
        raise RuntimeError('Remote builder compiler or Cargo does not match the pinned toolchain')
    return expected


def unique_run_id(worktree_id):
    # One launcher project owns both source and target paths. A per-build nonce
    # prevents another client with the same namespace from replacing either.
    return worktree_id[:31] + '-' + secrets.token_hex(16)


def cleanup_command(run_id):
    if not re.fullmatch(r'[a-zA-Z0-9][a-zA-Z0-9_-]{0,30}-[0-9a-f]{32}', run_id):
        raise ValueError('Refusing to clean an invalid remote run identifier')
    root='/mnt/runner-data/remote-build'
    source=shlex.quote(f'{root}/sources/{run_id}')
    target=shlex.quote(f'{root}/targets/{run_id}')
    archive=f'~/oxidex-remote-source-{run_id}.tar.gz'
    return f'sudo rm -rf -- {source} {target} && rm -f -- {archive}'


def source_sync_command(digest, upload, destination):
    archive = shlex.quote(upload)
    source = shlex.quote(destination)
    return (f"trap 'rm -f -- {archive}' EXIT; "
            f"echo '{digest}  {upload}' | sha256sum -c - "
            "|| { echo OXIDEX_SOURCE_CHECKSUM_MISMATCH >&2; exit 66; }; "
            f"tar -tzf {archive} >/dev/null "
            "|| { echo OXIDEX_SOURCE_ARCHIVE_INVALID >&2; exit 65; }; "
            f"find {source} -mindepth 1 -maxdepth 1 -exec rm -rf -- {{}} + && "
            f"tar -xzf {archive} -C {source}")


def retryable_exit(code):
    return code in (75, 137, 143, 255)


def retryable_failure(exc, stage):
    if stage == "cleanup":
        return False
    if stage in ('prepare', 'sync_upload', 'toolchain', 'verify', 'download'):
        return True
    if stage == 'sync_extract':
        # A checksum mismatch is a source integrity error. A lost SSH session
        # can be reported as exit 1 by gcloud, so all other extraction failures
        # are worker-specific and may use the next eligible VM.
        return (exc.returncode not in (65, 66) and
                not any(marker in (exc.stderr or '') for marker in (
                    'OXIDEX_SOURCE_CHECKSUM_MISMATCH', 'OXIDEX_SOURCE_ARCHIVE_INVALID')))
    return retryable_exit(exc.returncode)


def download_artifact(instance, zone, project, binary, artifact, digest):
    import tempfile
    import os
    fd,name=tempfile.mkstemp(prefix='.oxidex-download-',dir=artifact.parent)
    os.close(fd)
    temporary=Path(name)
    try:
        subprocess.run(['gcloud','compute','scp',instance+':'+binary,str(temporary),
            '--zone='+zone,'--project='+project,'--quiet','--scp-flag=-C',*SCP_KEEPALIVE],check=True)
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
            data=path.read_bytes()
            info=tar.gettarinfo(str(path),arcname=name)
            info.size=len(data)
            files.append({'path':name,'sha256':hashlib.sha256(data).hexdigest(),'bytes':len(data)})
            tar.addfile(info,io.BytesIO(data))
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
    parser.add_argument('--profile',choices=['debug','release','test'],default='release')
    parser.add_argument('--artifact-dir',type=Path)
    args=parser.parse_args(argv)
    import re
    if not re.fullmatch(r'[a-zA-Z0-9][a-zA-Z0-9_-]{0,63}',args.worktree_id):
        raise ValueError('Invalid worktree identifier')
    namespace=args.worktree_id
    args.worktree_id=unique_run_id(namespace)
    source=args.source.expanduser().resolve();evidence=args.evidence_dir.expanduser().resolve()
    if args.artifact_dir is None:
        args.artifact_dir=source/'target'/'remote-linux'/args.profile
    evidence.mkdir(parents=True,exist_ok=True)
    receipt={'profile':args.profile,'source':str(source),'instance':args.instance,'zone':args.zone,'project':args.project,
             'worktree_namespace':namespace,'run_id':args.worktree_id}
    def save():
        (evidence/'remote-build.json').write_text(json.dumps(receipt,indent=2))
    def ssh(command):
        return ['gcloud','compute','ssh',args.instance,'--zone='+args.zone,'--project='+args.project,
                '--quiet',*SSH_KEEPALIVE,'--command='+command]
    def cleanup(strict=False):
        try:
            subprocess.run(ssh(cleanup_command(args.worktree_id)),check=True)
            receipt['remote_cleanup']='complete'
        except Exception as cleanup_error:
            receipt['remote_cleanup']='failed'
            receipt['cleanup_error']=str(cleanup_error)
            save()
            if strict:
                raise
        save()
    archive=evidence/'remote-source.tar.gz'
    try:
        receipt['stage']='local_toolchain'
        receipt['toolchain']=pinned_toolchain(source)
        save()
        start=time.monotonic();archive=evidence/'remote-source.tar.gz'
        receipt['source_commit']=subprocess.check_output(['git','-C',str(source),'rev-parse','HEAD'],text=True).strip()
        receipt['source_status']=subprocess.check_output(['git','-C',str(source),'status','--porcelain'],text=True)
        if args.profile == 'test' and receipt['source_status']:
            raise RuntimeError('Remote workspace tests require a clean exact-HEAD checkout')
        receipt['snapshot']=make_snapshot(source,archive)
        receipt['packaging_seconds']=time.monotonic()-start
        after_commit=subprocess.check_output(['git','-C',str(source),'rev-parse','HEAD'],text=True).strip()
        after_status=subprocess.check_output(['git','-C',str(source),'status','--porcelain'],text=True)
        if after_commit!=receipt['source_commit'] or after_status!=receipt['source_status']:
            raise RuntimeError('Checkout metadata changed during snapshot; retry with a stable checkout')
        save()
        project=shlex.quote(args.worktree_id)
        receipt['stage']='prepare'
        subprocess.run(ssh(f'sudo /usr/local/bin/oxidex-remote-build {project} prepare'),check=True)
        receipt['stage']='sync_upload'
        start=time.monotonic()
        subprocess.run(['gcloud','compute','scp',str(archive),args.instance+':~/oxidex-remote-source-'+args.worktree_id+'.tar.gz',
                        '--zone='+args.zone,'--project='+args.project,'--quiet',*SCP_KEEPALIVE],check=True)
        digest=receipt['snapshot']['archive_sha256']
        destination='/mnt/runner-data/remote-build/sources/'+args.worktree_id
        upload='oxidex-remote-source-'+args.worktree_id+'.tar.gz'
        command=source_sync_command(digest,upload,destination)
        receipt['stage']='sync_extract'
        subprocess.run(ssh(command),check=True,capture_output=True,text=True)
        receipt['sync_seconds']=time.monotonic()-start;save()
        receipt['stage']='toolchain'
        verify_remote_toolchain(receipt['toolchain'], ssh, project)
        receipt['remote_toolchain_verified']=True
        save()
        # Dependencies are fetched separately; compile time excludes downloads.
        stages=[('fetch','cargo fetch --locked --target x86_64-unknown-linux-gnu')]
        if args.profile=='release':
            stages.append(('header','just cbindgen-check'))
        if args.profile=='test':
            stages.append(('test','cargo test --workspace --all-features --locked --no-fail-fast'))
        else:
            stages.append(('compile','cargo build '+('--release ' if args.profile=='release' else '')+'--locked --bin oxidex'))
        for stage,command in stages:
            receipt['stage']=stage
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
        if args.profile=='test':
            receipt['stage']='verify'
            if receipt.get('test_exit_code') != 0:
                raise RuntimeError('Remote workspace tests did not pass')
            receipt['stage']='cleanup'
            cleanup(strict=True)
            receipt['verified']=True
            save()
            print(json.dumps({k:v for k,v in receipt.items() if k!='snapshot'},indent=2))
            return 0
        binary=f'/mnt/runner-data/remote-build/targets/{args.worktree_id}/{args.profile}/oxidex'
        receipt['stage']='verify'
        verification=subprocess.check_output(ssh(shlex.quote(binary)+' --version && sha256sum '+shlex.quote(binary)),text=True)
        print(verification,flush=True)
        digest=verification.splitlines()[-1].split()[0]
        receipt['binary_sha256']=digest
        output=args.artifact_dir.expanduser().resolve()/args.worktree_id
        output.mkdir(parents=True,exist_ok=True)
        artifact=output/'oxidex'
        receipt['stage']='download'
        download_artifact(args.instance,args.zone,args.project,binary,artifact,digest)
        receipt['artifact']=str(artifact)
        receipt['verified']=True;save()
        receipt['stage']='cleanup'
        cleanup(strict=True)
    except Exception as exc:
        receipt['error']=str(exc)
        if isinstance(exc,subprocess.CalledProcessError):
            receipt['retryable']=retryable_failure(exc, receipt.get('stage'))
            if receipt.get('stage') == 'sync_extract':
                log=evidence/'sync-extract.log'
                log.write_text((exc.stdout or '') + (exc.stderr or ''))
                receipt['error']=f'Source extraction failed; see {log}'
                print(receipt['error'], file=sys.stderr, flush=True)
        save()
        if receipt.get('stage') not in ('local_toolchain','cleanup'):
            cleanup()
        raise
    finally:
        archive.unlink(missing_ok=True)
    print(json.dumps({k:v for k,v in receipt.items() if k!='snapshot'},indent=2))
    return 0
