"""Snapshot a local checkout, transfer it over authenticated SSH, and time builds."""
import argparse
import hashlib
import json
import os
import shlex
import re
import resource
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

sys.path.insert(0, str(Path(__file__).resolve().parents[3]))
from tools.release import approved_linux_perl


SSH_KEEPALIVE = ('--ssh-flag=-oServerAliveInterval=15',
                 '--ssh-flag=-oServerAliveCountMax=3')
SCP_KEEPALIVE = ('--scp-flag=-oServerAliveInterval=15',
                 '--scp-flag=-oServerAliveCountMax=3')
FLEET_RECIPES = frozenset({'fleet-test', 'fleet-tests-both', 'test-ignored',
                           'freeze-linux-perl', 'verify-linux-perl',
                           'prove-linux-perl-component', 'docs-site-build',
                           'test-remote-build'})
INFRA_PYTHON_PROFILE = 'infra-python-v1'
INFRA_PYTHON_RECIPE = 'infra-python-tests'
INFRA_PYTHON_ORIGINS = frozenset({
    'git@github.com:swack-tools/spot-github-runners.git',
    'https://github.com/swack-tools/spot-github-runners.git',
    'https://github.com/swack-tools/spot-github-runners',
})
INFRA_PYTHON_REQUIRED = frozenset({
    'justfile', 'rust-toolchain.toml', 'src/lib/host_admission.py',
    'src/lib/qualification_trust/control_engine.py',
    'tests/test_builder_c9_proof_repairs.py',
})
MAX_CANDIDATE_RECEIPT_BYTES = 64 * 1024
MAX_CANDIDATE_ARCHIVE_BYTES = 256 * 1024 * 1024


def source_git_env():
    """Read signed source identity and bytes without Git replacement refs."""
    return dict(os.environ, GIT_NO_REPLACE_OBJECTS='1')


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


def verify_signed_source(source, head, signer_path=None):
    if signer_path is not None:
        from qualification_source import _trusted_key
        _trusted_key(signer_path)
    command=['git','-C',str(source)]
    if signer_path is not None:
        command += ['-c','gpg.format=ssh', '-c','gpg.ssh.allowedSignersFile='+str(signer_path)]
    identity = subprocess.check_output(
        [*command,'log','-1','--format=%an|%ae|%cn|%ce|%G?|%GS',head],
        text=True, env=source_git_env()).strip().split('|')
    expected = ['swackhamer','swackhamer@users.noreply.github.com',
                'swackhamer','swackhamer@users.noreply.github.com',
                'G','swackhamer@users.noreply.github.com']
    if identity != expected:
        raise RuntimeError('Remote workspace tests require the signed maintainer HEAD')
    subprocess.run([*command,'verify-commit',head], check=True, env=source_git_env(),
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


def _sha256_file(path: Path) -> str:
    digest=hashlib.sha256()
    with path.open('rb') as stream:
        for chunk in iter(lambda: stream.read(1024*1024), b''):
            digest.update(chunk)
    return digest.hexdigest()


def _download_checked_candidate(transport, remote: str, local: Path, digest: str | None,
                                max_bytes: int, expected_size: int | None = None) -> None:
    """Persist bounded exact remote bytes; preserve failure evidence and project."""
    import tempfile
    if ((digest is not None and not re.fullmatch(r'[0-9a-f]{64}', digest))
            or local.exists() or local.is_symlink()
            or type(max_bytes) is not int or max_bytes <= 0
            or (expected_size is not None and
                (type(expected_size) is not int or not 0 < expected_size <= max_bytes))):
        raise RuntimeError('Perl candidate destination, digest, or bound is invalid')
    descriptor,name=tempfile.mkstemp(prefix='.perl-candidate-',dir=local.parent)
    os.close(descriptor)
    temporary=Path(name)
    try:
        try:
            # The limit is inherited by scp and its children, closing the
            # remote-stat-to-transfer race before local evidence is exhausted.
            subprocess.run(transport.scp(temporary,remote,download=True),check=True,
                           preexec_fn=lambda: resource.setrlimit(
                               resource.RLIMIT_FSIZE, (max_bytes, max_bytes)))
        except subprocess.CalledProcessError as exc:
            if temporary.stat().st_size >= max_bytes:
                raise RuntimeError('Perl candidate download bound exceeded') from exc
            raise
        size=temporary.stat().st_size
        if size > max_bytes:
            raise RuntimeError('Perl candidate download bound exceeded')
        if expected_size is not None and size != expected_size:
            raise RuntimeError('Downloaded Perl candidate size differs from receipt')
        if digest is not None and _sha256_file(temporary)!=digest:
            raise RuntimeError('Downloaded Perl candidate checksum mismatch')
        temporary.replace(local)
    except Exception:
        # Keep failure bytes for diagnosis, including errors from the child.
        if temporary.exists():
            if temporary.stat().st_size > max_bytes:
                with temporary.open('r+b') as stream:
                    stream.truncate(max_bytes)
            temporary.rename(temporary.with_name(temporary.name + '.failed'))
        raise


def retrieve_perl_candidate(transport, ssh, run_id: str, evidence: Path,
                            head: str, tree: str, bundle_sha256: str,
                            lock_path: Path) -> dict:
    """Download receipt and tar from the one retained project before PASS."""
    if transport is None:
        raise RuntimeError('Perl candidate requires authenticated direct transport')
    remote_root=f'/mnt/runner-data/remote-build/targets/{run_id}/perl-candidate-export'
    local=evidence/'linux-perl-candidate'
    local.mkdir(mode=0o700,exist_ok=False)
    receipt_remote=remote_root+'/candidate-receipt.json'
    receipt_path=local/'candidate-receipt.json'
    _download_checked_candidate(transport,receipt_remote,receipt_path,None,
                                MAX_CANDIDATE_RECEIPT_BYTES)
    receipt_digest=_sha256_file(receipt_path)
    candidate=json.loads(receipt_path.read_text())
    archive_remote=remote_root+'/perl-5.38.2-prefix.tar.gz'
    if (candidate.get('schema_version')!=1 or candidate.get('kind')!='linux_perl_unapproved_candidate'
            or candidate.get('source_head')!=head or candidate.get('source_tree')!=tree
            or candidate.get('source_clean_context')!='remote_verified_signed_fleet_checkout'
            or candidate.get('source_bundle_sha256')!=bundle_sha256
            or candidate.get('config_prefix')!='/target/ops/toolchains/perl-5.38.2/prefix'
            or candidate.get('archive_path')!='/target/ops/evidence/linux-perl-independent-identity/perl-5.38.2-prefix.tar.gz'
            or candidate.get('candidate_export_path')!='/target/perl-candidate-export'
            or candidate.get('lock_sha256')!=_sha256_file(lock_path)
            or candidate.get('status')!='candidate_only_requires_independent_review'
            or candidate.get('replay_tree_sha256')!=candidate.get('perl_tree_sha256')
            or not re.fullmatch(r'[0-9a-f]{64}',str(candidate.get('archive_sha256','')))
            or type(candidate.get('archive_bytes')) is not int
            or not 0<candidate['archive_bytes']<=MAX_CANDIDATE_ARCHIVE_BYTES):
        raise RuntimeError('Perl candidate receipt does not bind selected source and archive')
    archive_path=local/'perl-5.38.2-prefix.tar.gz'
    _download_checked_candidate(transport,archive_remote,archive_path,candidate['archive_sha256'],
                                candidate['archive_bytes'],
                                expected_size=candidate['archive_bytes'])
    if archive_path.stat().st_size!=candidate['archive_bytes']:
        raise RuntimeError('Downloaded Perl archive size differs from candidate receipt')
    return {'receipt':str(receipt_path),'receipt_sha256':receipt_digest,
            'archive':str(archive_path),'archive_sha256':candidate['archive_sha256'],
            'status':'unapproved_candidate_retrieved'}


def retrieve_component_proof(transport, ssh, run_id: str, evidence: Path,
                             expected: dict) -> dict:
    """Authenticate a small component-only receipt before reporting success."""
    if transport is None:
        raise RuntimeError('Component proof requires authenticated direct transport')
    remote=f'/mnt/runner-data/remote-build/targets/{run_id}/linux-perl-component-proof.json'
    local=evidence/'linux-perl-component-proof.json'
    _download_checked_candidate(transport,remote,local,None,MAX_CANDIDATE_RECEIPT_BYTES)
    digest=_sha256_file(local)
    proof=json.loads(local.read_text())
    if (not isinstance(proof,dict) or set(proof)!=set(expected)|{'prefix_mode','prefix_uid'}
            or any(proof.get(name)!=value for name,value in expected.items())
            or proof.get('prefix_mode') not in (0o700,0o755)
            or type(proof.get('prefix_uid')) is not int or proof['prefix_uid']<0):
        raise RuntimeError('Downloaded component proof does not bind approved source, packet and cold/warm result')
    return {'path':str(local),'sha256':digest,'kind':proof['kind'],
            'status':proof['status'],'cold':proof['cold'],'warm':proof['warm']}


def eligible_snapshot_paths(source: Path) -> list[str]:
    """Enumerate tracked plus nonignored untracked files under existing exclusions."""
    names = subprocess.check_output(['git','-C',str(source),'ls-files','-z',
                                     '--cached','--others','--exclude-standard'],
                                    env=source_git_env()).decode().split('\0')
    eligible=[]
    for name in names:
        parts=Path(name).parts
        if not name or any(part in {'.git','.codex','.claude','.agents','target','node_modules','.venv'}
                           for part in parts):
            continue
        path=source/name
        if (path.name.startswith('.env') or path.suffix in {'.pem','.key'}
                or not path.is_file() or path.is_symlink()):
            continue
        eligible.append(name)
    if len(eligible)!=len(set(eligible)):
        raise RuntimeError('Duplicate eligible source paths')
    return sorted(eligible)


def signed_snapshot_files(source: Path, head: str, *, source_profile=None) -> dict[str, tuple[str, int]]:
    """Enumerate fleet packet blobs and modes from the authenticated commit."""
    if not re.fullmatch(r'[0-9a-f]{40}', head):
        raise RuntimeError('Fleet source HEAD must be a full commit ID')
    env=source_git_env()
    tree=subprocess.check_output(['git','-C',str(source),'ls-tree','-r','-z',head],env=env)
    signed={}
    for entry in tree.split(b'\0'):
        if not entry:
            continue
        metadata, raw_name=entry.split(b'\t',1)
        mode, kind, object_id=metadata.decode('ascii').split()
        name=os.fsdecode(raw_name)
        parts=Path(name).parts
        if (not name or Path(name).is_absolute() or '..' in parts
                or any(part in {'.git','.codex','.claude','.agents','target','node_modules','.venv'}
                       for part in parts)
                or Path(name).name.startswith('.env')
                or Path(name).suffix in {'.pem','.key'}):
            continue
        if kind != 'blob' or mode not in {'100644','100755'} or name in signed:
            raise RuntimeError(f'Unsupported signed fleet source member: {name}')
        signed[name]=(object_id, 0o755 if mode=='100755' else 0o644)
    required={'justfile','rust-toolchain.toml','tools/remote-build/route.py',
              'tools/remote-build/qualification_bootstrap.py',
              'tools/remote-build/qualification_source.py',
              'tools/remote-build/test_runner.py','tools/release/bootstrap_oracle.py'}
    if source_profile == INFRA_PYTHON_PROFILE:
        required=INFRA_PYTHON_REQUIRED
    elif source_profile is not None:
        raise RuntimeError('Unsupported signed source profile')
    if required-signed.keys():
        label='Infrastructure source' if source_profile == INFRA_PYTHON_PROFILE else 'Signed fleet launcher'
        raise RuntimeError(f'{label} is incomplete: {sorted(required-signed.keys())}')
    return signed


def make_snapshot(source: Path, archive: Path, extra_files=None, *, signed_head=None, source_profile=None) -> dict:
    if source_profile is not None and signed_head is None:
        raise RuntimeError('Signed source profile requires exact HEAD')
    signed=signed_snapshot_files(source,signed_head,source_profile=source_profile) if signed_head is not None else None
    names=sorted(signed) if signed is not None else eligible_snapshot_paths(source)
    files=[]
    with tarfile.open(archive,'w:gz',compresslevel=3) as tar:
        for name in names:
            path=source/name
            if signed is None:
                if not path.is_file() or path.is_symlink():
                    raise RuntimeError('Source file changed type during snapshot')
                data=path.read_bytes()
                info=tar.gettarinfo(str(path),arcname=name)
            else:
                object_id, mode=signed[name]
                data=subprocess.check_output(
                    ['git','-C',str(source),'cat-file','blob',object_id],
                    env=source_git_env())
                if path.is_symlink() or not path.is_file() or path.read_bytes()!=data:
                    raise RuntimeError(f'Fleet source differs from signed HEAD: {name}')
                if bool(path.stat().st_mode & 0o111) != (mode==0o755):
                    raise RuntimeError(f'Fleet source mode differs from signed HEAD: {name}')
                info=tarfile.TarInfo(name)
                info.mode=mode
            info.size=len(data)
            row={'path':name,'sha256':hashlib.sha256(data).hexdigest(),'bytes':len(data)}
            if signed is not None:
                row['mode']=info.mode
            files.append(row)
            tar.addfile(info,io.BytesIO(data))
        for name,path in (extra_files or {}).items():
            if name in names or Path(name).name != name or path.is_symlink() or not path.is_file():
                raise RuntimeError('Invalid signed fleet source member')
            if name == 'approved-linux-perl.json':
                with path.open('rb') as stream:
                    data=stream.read(approved_linux_perl.MAX_ENVELOPE_BYTES + 1)
                if len(data)>approved_linux_perl.MAX_ENVELOPE_BYTES:
                    raise RuntimeError('Component envelope exceeds approved transport bound')
            else:
                data=path.read_bytes()
            info=tarfile.TarInfo(name)
            info.mode=0o644
            info.size=len(data)
            files.append({'path':name,'sha256':hashlib.sha256(data).hexdigest(),'bytes':len(data)})
            tar.addfile(info,io.BytesIO(data))
    if eligible_snapshot_paths(source)!=names:
        raise RuntimeError('Eligible source file set changed during snapshot')
    return {'files':files,'eligible_paths':names,'file_count':len(files),
            'archive_bytes':archive.stat().st_size,
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
    task=parser.add_mutually_exclusive_group()
    task.add_argument('--profile',choices=['debug','release','test'])
    task.add_argument('--just-recipe')
    parser.add_argument('--source-profile',choices=[INFRA_PYTHON_PROFILE])
    parser.add_argument('--just-arg',action='append',default=[])
    parser.add_argument('--approved-linux-perl-envelope',type=Path)
    parser.add_argument('--artifact-dir',type=Path)
    args=parser.parse_args(argv)
    if not args.profile and not args.just_recipe:
        args.profile='release'
    if args.just_arg and not args.just_recipe:
        parser.error('--just-arg requires --just-recipe')
    if args.just_recipe and not re.fullmatch(r'[a-z][a-z0-9_-]{0,63}',args.just_recipe):
        parser.error('invalid Just recipe name')
    infra_python=args.source_profile == INFRA_PYTHON_PROFILE
    if args.just_recipe == INFRA_PYTHON_RECIPE and not infra_python:
        parser.error('infra-python-tests requires --source-profile infra-python-v1')
    if infra_python and (args.just_recipe != INFRA_PYTHON_RECIPE or args.just_arg
                         or args.approved_linux_perl_envelope or args.artifact_dir):
        parser.error('infra-python-v1 requires only --just-recipe infra-python-tests with zero arguments')
    component=args.just_recipe=='prove-linux-perl-component'
    if bool(args.approved_linux_perl_envelope) != component or (component and args.just_arg):
        parser.error('component proof requires only its explicit approved envelope')
    if any(not value or len(value)>4096 or any(ch in value for ch in '\x00\n\r') for value in args.just_arg):
        parser.error('invalid Just argument')
    task_name=args.profile or 'recipe'
    explicit_identity=ssh_transport.identity()  # Refuse incomplete authentication before any subprocess.
    if args.just_recipe and (not explicit_identity or not os.environ.get('OXIDEX_REMOTE_SSH_KNOWN_HOSTS')):
        raise ValueError('Generic Just recipe requires explicit uploader user/key and trusted known-hosts file')
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
    envelope=None
    if component:
        envelope=args.approved_linux_perl_envelope.expanduser()
        if (not envelope.is_absolute() or envelope.is_symlink()
                or envelope.resolve(strict=False)!=envelope or not envelope.is_file()):
            raise RuntimeError('Component envelope must be an absolute regular file')
        if not 0<envelope.stat().st_size<=approved_linux_perl.MAX_ENVELOPE_BYTES:
            raise RuntimeError('Component envelope exceeds approved transport bound')
        if envelope.is_relative_to(source):
            raise RuntimeError('Component envelope must remain outside signed source checkout')
        envelope_sha256=_sha256_file(envelope)
    if args.artifact_dir is None:
        args.artifact_dir=source/'target'/'remote-linux'/task_name
    evidence.mkdir(parents=True,exist_ok=True)
    receipt={'profile':args.profile,'source_profile':args.source_profile,'just_recipe':args.just_recipe,'just_args':args.just_arg,'source':str(source),'instance':args.instance,'zone':args.zone,'project':args.project,
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
        receipt['source_commit']=subprocess.check_output(['git','-C',str(source),'rev-parse','HEAD'],
                                                        text=True,env=source_git_env()).strip()
        receipt['source_tree']=subprocess.check_output(['git','-C',str(source),'rev-parse','HEAD^{tree}'],
                                                      text=True,env=source_git_env()).strip()
        receipt['source_status']=subprocess.check_output(['git','-C',str(source),'status','--porcelain','--untracked-files=all'],
                                                        text=True,env=source_git_env())
        if args.just_recipe == 'freeze-linux-perl':
            source_tree=receipt['source_tree']
            if args.just_arg != [receipt['source_commit'], source_tree]:
                raise RuntimeError('Perl producer arguments differ from selected signed HEAD/tree')
        if args.profile == 'test' and receipt['source_status']:
            raise RuntimeError('Remote workspace tests require a clean exact-HEAD checkout')
        if args.profile == 'test':
            verify_signed_source(source, receipt['source_commit'])
        extra_files={}
        if args.just_recipe in FLEET_RECIPES or infra_python:
            if receipt['source_status']:
                raise RuntimeError('Signed remote recipe requires a clean exact-HEAD checkout')
            signer_path=Path(subprocess.check_output(
                ['git','-C',str(source),'config','--path','--get','gpg.ssh.allowedSignersFile'],
                text=True,env=source_git_env()).strip())
            if signer_path.is_symlink() or not signer_path.is_file():
                raise RuntimeError('Maintainer allowed signers file is unavailable')
            if infra_python:
                raw_origins=subprocess.check_output(
                    ['git','-C',str(source),'config','--local','--get-all','remote.origin.url'],
                    text=True,env=source_git_env()).splitlines()
                if len(raw_origins)!=1 or raw_origins[0] not in INFRA_PYTHON_ORIGINS:
                    raise RuntimeError('Infrastructure source raw origin is not the approved repository')
                receipt['source_origin_configured']=raw_origins[0]
                receipt['source_origin_check']='raw_local_config_allowlist'
                verify_signed_source(source, receipt['source_commit'], signer_path)
            else:
                verify_signed_source(source, receipt['source_commit'])
                # Fleet qualification also binds the OxiDex-specific source pins.
                from qualification_source import verify_source
                verify_source(source, receipt['source_commit'], signer_path)
            bundle=evidence/'repository.bundle'
            subprocess.run(['git','-C',str(source),'bundle','create',str(bundle),'HEAD'],check=True,
                           env=source_git_env())
            subprocess.run(['git','-C',str(source),'bundle','verify',str(bundle)],check=True,
                           env=source_git_env(),stdout=subprocess.DEVNULL)
            source_head=evidence/'fleet-source-head'
            source_head.write_text(receipt['source_commit']+'\n')
            extra_files={'repository.bundle':bundle,
                         'maintainer.allowed_signers':signer_path,
                         'fleet-source-head':source_head}
            if component:
                extra_files['approved-linux-perl.json']=envelope
        if infra_python:
            receipt['snapshot']=make_snapshot(source,archive,extra_files=extra_files,
                                              signed_head=receipt['source_commit'],
                                              source_profile=INFRA_PYTHON_PROFILE)
        elif extra_files:
            receipt['snapshot']=make_snapshot(source,archive,extra_files=extra_files,
                                              signed_head=receipt['source_commit'])
        else:
            receipt['snapshot']=make_snapshot(source,archive)
        if infra_python:
            receipt['source_provenance']='signed_exact_head_infra_python_v1'
            receipt['validation_scope']='infra_python_unittest_only'
        if extra_files:
            receipt['fleet_source_bundle_sha256']=next(
                row['sha256'] for row in receipt['snapshot']['files']
                if row['path']=='repository.bundle')
            if component:
                envelope_row=next(row for row in receipt['snapshot']['files']
                                  if row['path']=='approved-linux-perl.json')
                if (envelope_row['sha256']!=envelope_sha256
                        or envelope_row['bytes']!=envelope.stat().st_size
                        or _sha256_file(envelope)!=envelope_sha256):
                    raise RuntimeError('Component envelope changed during signed packet creation')
                receipt['component_envelope_sha256']=envelope_sha256
                receipt['component_envelope_bytes']=envelope_row['bytes']
        receipt['packaging_seconds']=time.monotonic()-start
        after_commit=subprocess.check_output(['git','-C',str(source),'rev-parse','HEAD'],
                                             text=True,env=source_git_env()).strip()
        after_status=subprocess.check_output(['git','-C',str(source),'status','--porcelain','--untracked-files=all'],
                                             text=True,env=source_git_env())
        if (after_commit!=receipt['source_commit'] or after_status!=receipt['source_status']
                or (receipt['snapshot'].get('eligible_paths') is not None
                    and eligible_snapshot_paths(source)!=receipt['snapshot']['eligible_paths'])
                or any(not (source/row['path']).is_file() or (source/row['path']).is_symlink()
                       or hashlib.sha256((source/row['path']).read_bytes()).hexdigest()!=row['sha256']
                       or (row.get('mode') is not None and
                           (0o755 if (source/row['path']).stat().st_mode & 0o111 else 0o644)
                           != row['mode'])
                       for row in receipt['snapshot'].get('files',[])
                       if row['path'] in receipt['snapshot'].get('eligible_paths', []))):
            raise RuntimeError('Checkout bytes changed during snapshot; retry with a stable checkout')
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
        if component:
            remote_envelope=destination+'/approved-linux-perl.json'
            observed=subprocess.check_output(ssh('sha256sum '+shlex.quote(remote_envelope)),text=True).split()[0]
            if observed!=envelope_sha256:
                raise RuntimeError('Extracted component envelope differs from signed packet')
            receipt['component_remote_envelope_sha256']=observed
            save()
        receipt['stage']='toolchain'
        verify_remote_toolchain(receipt['toolchain'], ssh, project)
        receipt['remote_toolchain_verified']=True
        save()
        # Fixed profiles fetch separately; generic Just recipes own their own Cargo behavior.
        stages=[] if args.just_recipe else [('fetch','cargo fetch --locked --target x86_64-unknown-linux-gnu')]
        if args.profile=='release':
            stages.append(('header','just cbindgen-check'))
        if args.just_recipe:
            recipe_args=([args.worktree_id] if component else args.just_arg)
            stages.append(('recipe',shlex.join(['just',args.just_recipe,*recipe_args])))
        elif args.profile=='test':
            stages.append(('test','python3 tools/remote-build/test_runner.py --source-sha '
                           +receipt['source_commit']+' --rustc-commit '+receipt['toolchain']['rustc_commit']))
        else:
            stages.append(('compile','cargo build '+('--release ' if args.profile=='release' else '')+'--locked --bin oxidex'))
        for stage,command in stages:
            receipt['stage']=stage
            start=time.monotonic()
            remote=f'sudo /usr/local/bin/oxidex-remote-build {project} {command}'
            if args.just_recipe and stage=='recipe':
                receipt['remote_retained']=True  # unknown child state on lost SSH acknowledgement
                receipt['remote_paths']={'source':'/mnt/runner-data/remote-build/sources/'+args.worktree_id,
                                         'target':'/mnt/runner-data/remote-build/targets/'+args.worktree_id}
                if args.just_recipe == 'docs-build':
                    receipt['rustdoc_path'] = receipt['remote_paths']['target'] + '/doc'
                if args.just_recipe == 'docs-site-build':
                    receipt['docs_site_snapshot_path'] = (
                        receipt['remote_paths']['target'] + '/docs-site')
                receipt['recipe_state']='RUNNING_OR_UNKNOWN'
                receipt['remote_command']=remote
                save()
            with (evidence/f'{stage}.log').open('w') as log:
                result=subprocess.run(ssh(remote),stdout=log,stderr=subprocess.STDOUT)
            receipt[stage+'_seconds']=time.monotonic()-start
            receipt[stage+'_exit_code']=result.returncode
            if args.just_recipe and stage=='recipe':
                log_path=evidence/'recipe.log'
                receipt['recipe_log_sha256']=hashlib.sha256(log_path.read_bytes()).hexdigest()
                receipt['recipe_log_bytes']=log_path.stat().st_size
                receipt['recipe_state']=('UNKNOWN_RETAINED' if result.returncode==255 else
                                         'DIRECT_SUCCESS_RETAINED' if result.returncode==0 else
                                         'DIRECT_FAILURE_RETAINED')
            receipt['retryable']=False if args.just_recipe else retryable_exit(result.returncode)
            if args.just_recipe and result.returncode:
                receipt['remote_retained']=True
                receipt['remote_paths']={'source':'/mnt/runner-data/remote-build/sources/'+args.worktree_id,
                                         'target':'/mnt/runner-data/remote-build/targets/'+args.worktree_id}
            save()
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
        if args.just_recipe:
            receipt['stage']='verify'
            binary_kind={'build':'debug','build-bin':'debug',
                         'build-release-local':'release','build-bin-release':'release'}.get(args.just_recipe)
            if binary_kind:
                binary='/mnt/runner-data/remote-build/targets/'+args.worktree_id+'/'+binary_kind+'/oxidex'
                digest=subprocess.check_output(ssh('sha256sum '+shlex.quote(binary)),text=True).split()[0]
                if not re.fullmatch(r'[0-9a-f]{64}',digest):
                    raise RuntimeError('Remote build artifact has no SHA-256')
                output=source/'target'/'remote-linux'/binary_kind/args.worktree_id
                output.mkdir(parents=True,exist_ok=True)
                artifact=output/'oxidex'
                download_artifact(args.instance,args.zone,args.project,binary,artifact,digest,
                                  **({'transport':transport} if transport else {}))
                receipt['binary_sha256']=digest
                receipt['artifact']=str(artifact)
            if args.just_recipe == 'freeze-linux-perl':
                receipt['candidate_retrieval']=retrieve_perl_candidate(
                    transport,ssh,args.worktree_id,evidence,receipt['source_commit'],
                    args.just_arg[1],receipt['fleet_source_bundle_sha256'],
                    source/'tools/release/oracle-lock.json')
            if component:
                descriptor_path=source/'tools/release/oracle-linux-perl-identity.json'
                descriptor=json.loads(descriptor_path.read_text())
                expected={'schema':1,'kind':'linux_perl_component_proof',
                          'status':'COMPONENT_ONLY_PASS','run_id':args.worktree_id,
                          'source_head':receipt['source_commit'],
                          'source_tree':subprocess.check_output(
                              ['git','-C',str(source),'rev-parse','HEAD^{tree}'],
                              text=True,env=source_git_env()).strip(),
                          'bundle_sha256':receipt['fleet_source_bundle_sha256'],
                          'descriptor_sha256':_sha256_file(descriptor_path),
                          'lock_sha256':_sha256_file(source/'tools/release/oracle-lock.json'),
                          'envelope_sha256':envelope_sha256,
                          'archive_sha256':descriptor['archive_sha256'],
                          'archive_bytes':descriptor['archive_bytes'],
                          'tree_sha256':descriptor['tree_sha256'],
                          'exe_sha256':descriptor['exe_sha256'],
                          'zip_sha256':descriptor['zip_sha256'],
                          'prefix':descriptor['prefix'],'config_prefix':descriptor['prefix'],
                          'zip_version':'1.68','cold':'installed','warm':'reused'}
                receipt['component_proof']=retrieve_component_proof(transport,ssh,args.worktree_id,evidence,expected)
                receipt['stage']='cleanup'
                save()
                cleanup(strict=True)
                # The ordinary cleanup removes the source project but retains
                # the target containing the verified component proof.
                receipt['remote_source']='removed'
                receipt['remote_retained']=True  # The proof-bearing target remains.
                receipt['remote_paths']={'target':'/mnt/runner-data/remote-build/targets/'+args.worktree_id}
                receipt['recipe_state']='DIRECT_SUCCESS_TARGET_RETAINED'
            receipt['verified']=True
            receipt['stage']='complete_target_retained' if component else 'complete_retained'
            save()
            print(json.dumps({k:v for k,v in receipt.items() if k!='snapshot'},indent=2))
            return 0
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
