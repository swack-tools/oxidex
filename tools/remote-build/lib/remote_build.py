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

from .config import approved_instances, builder_instance_name, matching_approval
from . import ssh_transport


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


def verify_signed_source(source, head):
    identity = subprocess.check_output(
        ['git','-C',str(source),'log','-1','--format=%an|%ae|%cn|%ce|%G?|%GS',head],
        text=True).strip().split('|')
    expected = ['swackhamer','swackhamer@users.noreply.github.com',
                'swackhamer','swackhamer@users.noreply.github.com',
                'G','swackhamer@users.noreply.github.com']
    if identity != expected:
        raise RuntimeError('Remote workspace tests require the signed maintainer HEAD')
    subprocess.run(['git','-C',str(source),'verify-commit',head], check=True,
                   stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)

def download_test_proof(instance, zone, project, remote, local, digest, expected_commit,
                        expected_toolchain, expected_oracle, *, require_pass=True, transport=None):
    import tempfile
    import os
    fd, name = tempfile.mkstemp(prefix='.oxidex-test-proof-', dir=local.parent)
    os.close(fd)
    temporary = Path(name)
    try:
        command = (transport.scp(temporary, remote, download=True) if transport else
            ['gcloud','compute','scp',ssh_transport.target(instance)+':'+remote,str(temporary),
             '--zone='+zone,'--project='+project,'--quiet',*SCP_KEEPALIVE,
             *ssh_transport.flags('scp')])
        subprocess.run(command,check=True)
        if hashlib.sha256(temporary.read_bytes()).hexdigest() != digest:
            raise RuntimeError('Downloaded remote test proof checksum mismatch')
        proof = json.loads(temporary.read_text())
        if (proof.get('schema') != 1 or proof.get('kind') != 'oxidex_spot_workspace_test'
                or proof.get('source_commit') != expected_commit
                or proof.get('rust_pin') != expected_toolchain['channel']
                or proof.get('rustc_commit') != expected_toolchain['rustc_commit']
                or proof.get('cargo_version') != expected_toolchain['cargo_version']
                or not isinstance(proof.get('rustc_version'), str)
                or re.findall(r'^commit-hash: ([0-9a-f]{40})$', proof['rustc_version'], re.M)
                   != [expected_toolchain['rustc_commit']]
                or re.findall(r'^release: (\S+)$', proof['rustc_version'], re.M)
                   != [expected_toolchain['channel']]
                or proof.get('oracle_pin') != expected_oracle
                or not re.fullmatch(r'[0-9a-f]{64}', proof.get('bootstrap_manifest_sha256', ''))
                or not re.fullmatch(r'[0-9a-f]{64}', proof.get('perl_sha256', ''))
                or not re.fullmatch(r'[0-9a-f]{64}', proof.get('exiftool_tree_sha256', ''))
                or not re.fullmatch(r'[0-9a-f]{64}', proof.get('corpus_tree_sha256', ''))
                or proof.get('corpus_files', 0) < 4000):
            raise RuntimeError('Remote test proof does not establish pinned exact-head PASS')
        if (not isinstance(proof.get('python_command'), list)
                or proof['python_command'][1:] != ['-m', 'unittest', 'discover', '-s', 'tests', '-p', 'test_*.py']
                or (proof.get('qualification_unit_command') is not None and
                    (not isinstance(proof['qualification_unit_command'], list) or
                     proof['qualification_unit_command'][1:] != ['-m', 'unittest', 'test_version_transition_qualification.py']))
                or (proof.get('test_command') is not None and
                    proof['test_command'] != ['cargo', 'test', '--workspace', '--all-features', '--locked', '--no-fail-fast'])):
            raise RuntimeError('Remote test proof has an unexpected command')
        if require_pass:
            if (proof.get('status') != 'PASS' or proof.get('test_exit_code') != 0
                    or proof.get('python_exit_code') != 0 or proof.get('qualification_unit_exit_code') != 0
                    or proof.get('test_command') is None or proof.get('qualification_unit_command') is None):
                raise RuntimeError('Remote test proof does not establish pinned exact-head PASS')
        elif proof.get('status') != 'FAILED' or not isinstance(proof.get('test_exit_code'), int) or proof['test_exit_code'] == 0:
            raise RuntimeError('Remote test proof does not establish a bound failure')
        temporary.replace(local)
        return proof
    finally:
        temporary.unlink(missing_ok=True)


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
    archive=f'~/oxidex-remote-source-{run_id}.tar.gz'
    return f'sudo /usr/local/bin/oxidex-remote-build {shlex.quote(run_id)} cleanup && rm -f -- {archive}'



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


def download_artifact(instance, zone, project, binary, artifact, digest, transport=None):
    import tempfile
    import os
    fd,name=tempfile.mkstemp(prefix='.oxidex-download-',dir=artifact.parent)
    os.close(fd)
    temporary=Path(name)
    try:
        command = (transport.scp(temporary, binary, download=True) if transport else
            ['gcloud','compute','scp',ssh_transport.target(instance)+':'+binary,str(temporary),
             '--zone='+zone,'--project='+project,'--quiet','--scp-flag=-C',*SCP_KEEPALIVE,
             *ssh_transport.flags('scp')])
        subprocess.run(command,check=True)
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


class BuilderBusy(RuntimeError):
    """Transient host utilization refusal, before any source mutation."""


def explicit_resource_probe(ssh):
    """Read native Linux CPU/memory before an explicit bootstrap dispatch.

    This is a current-host admission probe, not a Monitoring window or release
    qualification. The root launcher still owns atomic container admission.
    """
    script = """import json,time
from pathlib import Path
def cpu():
    values=[int(v) for v in Path('/proc/stat').read_text().splitlines()[0].split()[1:9]]
    return sum(values),values[3]+values[4]
a=cpu();time.sleep(1);b=cpu()
delta=b[0]-a[0]
if delta<=0:raise SystemExit('Invalid CPU observation')
mem={line.split(':')[0]:int(line.split()[1]) for line in Path('/proc/meminfo').read_text().splitlines() if line.startswith(('MemTotal:','MemAvailable:'))}
print(json.dumps({'cpu':1-(b[1]-a[1])/delta,'memory':1-mem['MemAvailable']/mem['MemTotal'],'observed_at':time.time(),'method':'native-linux-one-second'}))
"""
    output=subprocess.check_output(ssh('python3 -c '+shlex.quote(script)),text=True,timeout=15)
    record=json.loads(output)
    import math
    if (not isinstance(record,dict) or record.get('method')!='native-linux-one-second'
            or any(type(record.get(k)) not in (int,float) or not math.isfinite(record[k])
                   or not 0<=record[k]<=1 for k in ('cpu','memory'))
            or type(record.get('observed_at')) not in (int,float)
            or not math.isfinite(record['observed_at'])
            or abs(time.time()-record['observed_at'])>15):
        raise RuntimeError('Explicit builder CPU/memory admission refused')
    if any(record[k]>=.75 for k in ('cpu','memory')):
        raise BuilderBusy('Explicit builder CPU/memory admission refused: busy host')
    return record



def verify_builder_admission(instance, zone, project, instance_id, ssh):
    from .worker_selection import launcher_probe
    actual = json.loads(subprocess.check_output([
        'gcloud', 'compute', 'instances', 'describe', instance,
        '--zone='+zone, '--project='+project, '--format=json(id,status)'], text=True))
    if str(actual.get('id')) != instance_id or actual.get('status') != 'RUNNING':
        raise RuntimeError('Pinned remote builder identity changed or is not running')
    subprocess.run(ssh('sh -c '+shlex.quote(launcher_probe())), check=True, timeout=60)
    resource = explicit_resource_probe(ssh)
    return {'instance_id':instance_id,'launcher_verified':True,
            'resource_probe':resource,'admission_passed':True}


def main(argv=None):
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--source',type=Path,required=True)
    parser.add_argument('--instance',required=True)
    parser.add_argument('--zone',required=True)
    parser.add_argument('--instance-id')
    parser.add_argument('--project',default='homelab-424523')
    parser.add_argument('--worktree-id',required=True)
    parser.add_argument('--evidence-dir',type=Path,required=True)
    parser.add_argument('--profile',choices=['debug','release','test'],default='release')
    parser.add_argument('--artifact-dir',type=Path)
    args=parser.parse_args(argv)
    ssh_transport.identity()  # Refuse incomplete authentication before any subprocess.
    if args.instance_id is not None and not re.fullmatch(r'[0-9]{1,20}', args.instance_id):
        raise ValueError('Invalid pinned instance ID')
    approval = None
    if not builder_instance_name(args.instance):
        approval = matching_approval(approved_instances(), args.project, args.instance, args.zone)
        if approval is None:
            raise ValueError('Remote builds require a dedicated builder-* VM or an explicit identity-bound approval')
    if args.instance_id is None and approval is None:
        raise ValueError('Direct builder requires a pinned instance ID before remote work')
    if not re.fullmatch(r'[a-zA-Z0-9][a-zA-Z0-9_-]{0,63}',args.worktree_id):
        raise ValueError('Invalid worktree identifier')
    if args.instance_id is None:
        args.instance_id = approval['id']
    namespace=args.worktree_id
    args.worktree_id=unique_run_id(namespace)
    source=args.source.expanduser().resolve();evidence=args.evidence_dir.expanduser().resolve()
    if args.artifact_dir is None:
        args.artifact_dir=source/'target'/'remote-linux'/args.profile
    evidence.mkdir(parents=True,exist_ok=True)
    receipt={'profile':args.profile,'source':str(source),'instance':args.instance,'zone':args.zone,'project':args.project,
             'worktree_namespace':namespace,'run_id':args.worktree_id}
    if approval:
        receipt['approved_instance'] = approval
    def save():
        (evidence/'remote-build.json').write_text(json.dumps(receipt,indent=2))
    if approval:
        from .worker_selection import select_worker
        receipt['stage'] = 'admission'
        try:
            actual = json.loads(subprocess.check_output([
                'gcloud', 'compute', 'instances', 'describe', args.instance,
                '--zone='+args.zone, '--project='+args.project, '--format=json(id,status)'], text=True))
            if str(actual.get('id')) != approval['id'] or actual.get('status') != 'RUNNING':
                raise RuntimeError('Approved remote builder identity changed or is not running')
            select_worker(args.project, required_name=args.instance,
                          required_id=approval['id'], required_zone=args.zone)
        except (RuntimeError, subprocess.CalledProcessError, subprocess.TimeoutExpired) as exc:
            receipt.update(error=str(exc), retryable=True)
            save()
            raise
    transport = None
    if ssh_transport.identity():
        receipt['stage'] = 'transport_identity'
        save()
        try:
            transport = ssh_transport.DirectTransport(args.instance, args.zone, args.project,
                args.instance_id or (approval['id'] if approval else None))
            receipt['transport'] = {'method':'direct-ssh', 'instance_id':transport.instance_id,
                                    'address':transport.host}
            save()
        except Exception as exc:
            receipt.update(error=str(exc), retryable=isinstance(exc,
                (subprocess.CalledProcessError, subprocess.TimeoutExpired)))
            save()
            raise
    def ssh(command):
        if transport:
            return transport.ssh(command)
        return ['gcloud','compute','ssh',ssh_transport.target(args.instance),'--zone='+args.zone,'--project='+args.project,
                '--quiet',*SSH_KEEPALIVE,*ssh_transport.flags('ssh'),'--command='+command]
    if args.instance_id is not None:
        receipt['stage'] = 'identity_probe'
        receipt['admission_passed'] = False
        save()
        try:
            receipt.update(verify_builder_admission(args.instance, args.zone, args.project, args.instance_id, ssh))
            save()
        except Exception as exc:
            transient = (isinstance(exc, (BuilderBusy, subprocess.TimeoutExpired))
                         or isinstance(exc, subprocess.CalledProcessError) and exc.returncode in (75, 255))
            receipt.update(error=str(exc), retryable=transient, admission_passed=False)
            save()
            raise
    def cleanup(strict=False):
        try:
            subprocess.run(ssh(cleanup_command(args.worktree_id)),check=True)
            receipt['remote_cleanup']='complete'
            receipt['remote_targets']='retained'
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
        if args.profile == 'test':
            verify_signed_source(source, receipt['source_commit'])
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
        upload_path='~/oxidex-remote-source-'+args.worktree_id+'.tar.gz'
        upload_command = (transport.scp(archive, upload_path) if transport else
            ['gcloud','compute','scp',str(archive),ssh_transport.target(args.instance)+':'+upload_path,
             '--zone='+args.zone,'--project='+args.project,'--quiet',*SCP_KEEPALIVE,
             *ssh_transport.flags('scp')])
        subprocess.run(upload_command,check=True)
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
            stages.append(('test','python3 tools/remote-build/test_runner.py --source-sha '
                           +receipt['source_commit']+' --rustc-commit '+receipt['toolchain']['rustc_commit']))
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
                if stage == 'test':
                    remote_proof=f'/mnt/runner-data/remote-build/targets/{args.worktree_id}/remote-test.json'
                    try:
                        remote_hash=subprocess.check_output(ssh('sha256sum '+shlex.quote(remote_proof)),text=True).split()[0]
                        if not re.fullmatch(r'[0-9a-f]{64}',remote_hash):
                            raise RuntimeError('Remote failed-test proof has no SHA-256')
                        receipt['test_proof']=download_test_proof(args.instance,args.zone,args.project,
                            remote_proof,evidence/'remote-test.json',remote_hash,receipt['source_commit'],
                            receipt['toolchain'],(source/'.exiftool-version').read_text().strip(),require_pass=False,transport=transport)
                        receipt['test_proof_sha256']=remote_hash
                    except Exception as proof_error:
                        # The command may have failed before writing a proof or
                        # SSH may have lost its result. Do not destroy evidence
                        # whose process state and payload are unconfirmed.
                        receipt['remote_retained']=True
                        receipt['retryable']=False
                        receipt['remote_paths']={'source':'/mnt/runner-data/remote-build/sources/'+args.worktree_id,
                                                 'target':'/mnt/runner-data/remote-build/targets/'+args.worktree_id}
                        receipt['failure_proof_error']=str(proof_error)
                    save()
                raise RuntimeError(f'{stage} failed; see {evidence / (stage+".log")}')
        if args.profile=='test':
            receipt['stage']='verify'
            if receipt.get('test_exit_code') != 0:
                raise RuntimeError('Remote workspace tests did not pass')
            remote_proof=f'/mnt/runner-data/remote-build/targets/{args.worktree_id}/remote-test.json'
            # A completed test is not a verified test until its proof reaches
            # durable local evidence. Retain remote paths across SSH/download
            # failures instead of deleting the only completed proof.
            receipt['remote_retained']=True
            receipt['remote_paths']={'source':'/mnt/runner-data/remote-build/sources/'+args.worktree_id,
                                     'target':'/mnt/runner-data/remote-build/targets/'+args.worktree_id}
            save()
            remote_hash=subprocess.check_output(ssh('sha256sum '+shlex.quote(remote_proof)),text=True).split()[0]
            if not re.fullmatch(r'[0-9a-f]{64}',remote_hash):
                raise RuntimeError('Remote test proof has no SHA-256')
            local_proof=evidence/'remote-test.json'
            receipt['test_proof']=download_test_proof(args.instance,args.zone,args.project,
                remote_proof,local_proof,remote_hash,receipt['source_commit'],receipt['toolchain'],
                (source/'.exiftool-version').read_text().strip(),transport=transport)
            receipt['test_proof_sha256']=remote_hash
            receipt['remote_retained']=False
            receipt.pop('remote_paths',None)
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
        download_artifact(args.instance,args.zone,args.project,binary,artifact,digest,
                          **({'transport':transport} if transport else {}))
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
        if receipt.get('stage') not in ('local_toolchain','cleanup') and not receipt.get('remote_retained'):
            cleanup()
        raise
    finally:
        archive.unlink(missing_ok=True)
    print(json.dumps({k:v for k,v in receipt.items() if k!='snapshot'},indent=2))
    return 0
