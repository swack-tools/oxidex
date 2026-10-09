"""Fixed GitHub authority plus raw local ancestry for unpublished infra candidates.

Receipts record an online controller decision; caller-provided receipts never
serve as admission authority. This proves lineage, not prior publication.
"""
import grp
import hashlib
import json
import math
import os
from pathlib import Path
import pwd
import re
import selectors
import stat
import subprocess
import sys
import time

REPOSITORY = 'swack-tools/spot-github-runners'
REPOSITORY_ID = 1397480856
REPO_ENDPOINT = '/repos/' + REPOSITORY
REF_ENDPOINT = REPO_ENDPOINT + '/git/ref/heads/main'
REF = 'refs/heads/main'
MAX_API_BYTES = 256 * 1024
MAX_COMMIT_BYTES = 1024 * 1024
MAX_TOTAL_BYTES = 16 * 1024 * 1024
MAX_COMMITS = 4096
MAX_PARENTS = 32
MAX_GRAPH_SECONDS = 30
HEX = re.compile(r'[0-9a-f]{40}\Z')


def _bounded_command(command, env, limit, seconds, *, data=None, pass_fds=()):
    """Cap bytes during receipt, kill/reap on error, never expose tool stderr."""
    if seconds <= 0:
        raise RuntimeError('Repository authority deadline exceeded')
    deadline = time.monotonic() + seconds
    process = subprocess.Popen(command, stdin=subprocess.PIPE if data is not None else subprocess.DEVNULL,
                               stdout=subprocess.PIPE, stderr=subprocess.DEVNULL, env=env, cwd='/',
                               pass_fds=pass_fds)
    try:
        if data is not None:
            # Only a short object ID or one bounded token header; never file input.
            if len(data) > 8192:
                raise RuntimeError('Repository authority input limit exceeded')
            process.stdin.write(data)
            process.stdin.close()
        chunks = []
        size = 0
        with selectors.DefaultSelector() as selector:
            selector.register(process.stdout, selectors.EVENT_READ)
            while True:
                remaining = deadline - time.monotonic()
                if remaining <= 0 or not selector.select(remaining):
                    raise RuntimeError('Repository authority deadline exceeded')
                part = os.read(process.stdout.fileno(), min(65536, limit + 1 - size))
                if not part:
                    break
                chunks.append(part)
                size += len(part)
                if size > limit:
                    raise RuntimeError('Repository authority output limit exceeded')
        remaining = deadline - time.monotonic()
        if remaining <= 0:
            raise RuntimeError('Repository authority deadline exceeded')
        if process.wait(timeout=remaining) != 0:
            raise RuntimeError('Repository authority tool refused')
        return b''.join(chunks)
    except (OSError, subprocess.TimeoutExpired) as error:
        raise RuntimeError('Repository authority tool unavailable') from None
    finally:
        if process.poll() is None:
            process.kill()
        process.wait()
        process.stdout.close()
        if process.stdin is not None and not process.stdin.closed:
            process.stdin.close()


def _trusted_tool(path, *, user_owned=False):
    selected = Path(path).resolve(strict=True)
    owners = {0, os.geteuid()} if user_owned else {0}
    if user_owned:
        # The existing maintainer gh installation is trusted, never a checkout
        # helper or arbitrary PATH executable. Homebrew symlinks must resolve
        # to the fixed gh package shape, not another owner-writable location.
        native = str(selected) in ('/usr/bin/gh', '/usr/local/bin/gh')
        brew = re.fullmatch(r'/(?:opt/homebrew|usr/local)/Cellar/gh/[^/]+/bin/gh', str(selected))
        if not native and not brew:
            raise RuntimeError('Untrusted repository credential tool location')
    for component in (*reversed(selected.parents), selected):
        info = component.lstat()
        kind = stat.S_ISREG if component == selected else stat.S_ISDIR
        # Explicit policy: the maintainer's macOS administrators are trusted.
        # Only the standard Cellar directory may have this exact admin-write
        # mode; packages, binaries and every other ancestor remain nonwritable
        # by group/others. This does not authorize repository-controlled tools.
        cellar_admin = False
        if (user_owned and sys.platform == 'darwin' and component != selected
                and str(component) in ('/opt/homebrew/Cellar', '/usr/local/Cellar')
                and info.st_gid == 80 and stat.S_IMODE(info.st_mode) == 0o775):
            try:
                cellar_admin = grp.getgrgid(80).gr_name == 'admin'
            except KeyError:
                pass
        if (not kind(info.st_mode) or info.st_uid not in owners
                or (info.st_mode & 0o022 and not cellar_admin)):
            raise RuntimeError('Untrusted repository authority tool')
    if not selected.stat().st_mode & 0o111:
        raise RuntimeError('Repository authority tool is not executable')
    return str(selected)


def _maintainer_token():
    # Fixed locations, never caller PATH. Credential storage belongs to the OS
    # account, not HOME/GH_CONFIG_DIR supplied by a checkout or ambient wrapper.
    candidates = ('/opt/homebrew/bin/gh', '/usr/bin/gh', '/usr/local/bin/gh')
    selected = next((p for p in candidates if Path(p).exists()), None)
    if selected is None:
        raise RuntimeError('Maintainer GitHub credential tool unavailable')
    tool = _trusted_tool(selected, user_owned=True)
    env = {'PATH':'/usr/bin:/bin', 'HOME':pwd.getpwuid(os.geteuid()).pw_dir,
           'LANG':'C', 'GH_PROMPT_DISABLED':'1'}
    raw = _bounded_command([tool, 'auth', 'token', '--hostname', 'github.com',
                            '--user', 'swackhamer'], env, 4096, 5)
    try:
        token = raw.decode('ascii').strip()
    except UnicodeError:
        raise RuntimeError('Maintainer GitHub credential unavailable') from None
    if not re.fullmatch(r'[A-Za-z0-9_-]{1,4096}', token):
        raise RuntimeError('Maintainer GitHub credential unavailable')
    return token


def _api(endpoint, token):
    if endpoint not in ('/user', REPO_ENDPOINT, REF_ENDPOINT):
        raise RuntimeError('Repository authority endpoint is not fixed')
    if not re.fullmatch(r'[A-Za-z0-9_-]{1,4096}', token):
        raise RuntimeError('Repository authority credential malformed')
    # -q is first: no .curlrc; no proxy/env CA/host overrides or redirects. The
    # secret header is stdin, never argv, saved config, receipt or stderr.
    tool = _trusted_tool('/usr/bin/curl')
    command = [tool, '-q', '--silent', '--fail', '--noproxy', '*',
               '--proto', '=https', '--proto-redir', '=https', '--max-redirs', '0',
               '--connect-timeout', '5', '--max-time', '10', '--max-filesize', str(MAX_API_BYTES),
               '--header', 'Accept: application/vnd.github+json',
               '--header', 'X-GitHub-Api-Version: 2022-11-28',
               '--config', '-', '--write-out', '\n%{http_code}',
               'https://api.github.com' + endpoint]
    raw = _bounded_command(command, {'PATH':'/usr/bin:/bin', 'HOME':'/nonexistent', 'LANG':'C'},
                           MAX_API_BYTES + 4, 11,
                           data=('header = "Authorization: Bearer ' + token + '"\n').encode())
    if len(raw) < 4 or raw[-4:] != b'\n200':
        raise RuntimeError('Repository authority HTTP response refused')
    return raw[:-4]


def _json(raw):
    def pairs(rows):
        value = {}
        for key, item in rows:
            if key in value:
                raise ValueError('duplicate key')
            value[key] = item
        return value
    def invalid(_):
        raise ValueError('nonfinite JSON')
    def number(raw):
        value = float(raw)
        if not math.isfinite(value):
            raise ValueError('nonfinite JSON')
        return value
    try:
        if len(raw) > MAX_API_BYTES:
            raise ValueError('body limit')
        result = json.loads(raw, object_pairs_hook=pairs, parse_constant=invalid, parse_float=number)
        if not isinstance(result, dict):
            raise ValueError('object required')
        return result
    except (ValueError, UnicodeError, RecursionError):
        raise RuntimeError('Repository authority JSON refused') from None


def observe():
    token = _maintainer_token()
    documents = {}
    hashes = {}
    responses = {}
    started = time.monotonic()
    for endpoint in ('/user', REPO_ENDPOINT, REF_ENDPOINT):
        raw = _api(endpoint, token)
        documents[endpoint] = _json(raw)
        if endpoint == '/user' and documents[endpoint].get('login') != 'swackhamer':
            raise RuntimeError('Repository authority identity refused')
        hashes[endpoint] = hashlib.sha256(raw).hexdigest()
        if endpoint != '/user':
            try:
                responses[endpoint] = raw.decode('utf-8')
            except UnicodeError:
                raise RuntimeError('Repository authority JSON encoding refused') from None
    user, repo, ref = (documents[p] for p in ('/user', REPO_ENDPOINT, REF_ENDPOINT))
    obj = ref.get('object')
    if (user.get('login') != 'swackhamer' or type(repo.get('id')) is not int
            or repo['id'] != REPOSITORY_ID or repo.get('full_name') != REPOSITORY
            or repo.get('default_branch') != 'main' or repo.get('archived') is not False
            or repo.get('fork') is not False or ref.get('ref') != REF
            or not isinstance(obj, dict) or obj.get('type') != 'commit'
            or not isinstance(obj.get('sha'), str) or not HEX.fullmatch(obj['sha'])):
        raise RuntimeError('Repository authority identity refused')
    return {'repository':REPOSITORY, 'repository_id':REPOSITORY_ID, 'ref':REF,
            'sha':obj['sha'], 'authenticated_login':'swackhamer',
            'observed_at':time.time(), 'elapsed_seconds':time.monotonic()-started,
            'response_sha256':hashes, 'response_json':responses}


def parse_commit(oid, raw):
    if hashlib.sha1(b'commit ' + str(len(raw)).encode() + b'\0' + raw).hexdigest() != oid:
        raise RuntimeError('Raw commit object hash differs')
    header, separator, _ = raw.partition(b'\n\n')
    if not separator or b'\0' in header or b'\r' in header:
        raise RuntimeError('Raw commit header refused')
    parents = []
    tree = None
    previous = None
    for line in header.split(b'\n'):
        if line.startswith(b' '):
            if previous not in (b'gpgsig', b'gpgsig-sha256', b'mergetag'):
                raise RuntimeError('Raw commit continuation refused')
            continue
        key, sep, value = line.partition(b' ')
        if not sep or not re.fullmatch(rb'[A-Za-z0-9-]{1,128}', key):
            raise RuntimeError('Raw commit header refused')
        previous = key
        if key in (b'tree', b'parent'):
            if not re.fullmatch(rb'[0-9a-f]{40}', value):
                raise RuntimeError('Raw commit tree/parent refused')
            value = value.decode('ascii')
            if key == b'tree':
                if tree is not None:
                    raise RuntimeError('Raw commit duplicate tree refused')
                tree = value
            else:
                if value in parents or len(parents) >= MAX_PARENTS:
                    raise RuntimeError('Raw commit parent limit/duplicate refused')
                parents.append(value)
    if tree is None:
        raise RuntimeError('Raw commit tree missing')
    return {'oid':oid, 'tree':tree, 'parents':parents, 'bytes':len(raw),
            'raw_sha256':hashlib.sha256(raw).hexdigest()}


def _raw_commit(source, oid, env, deadline, *, byte_budget=MAX_TOTAL_BYTES):
    if not HEX.fullmatch(oid):
        raise RuntimeError('Raw commit identity refused')
    command = ['/usr/bin/git', '-C', str(source), 'cat-file']
    info = _bounded_command(command + ['--batch-check=%(objectname) %(objecttype) %(objectsize)'],
                            env, 128, deadline-time.monotonic(), data=(oid+'\n').encode())
    match = re.fullmatch(rb'([0-9a-f]{40}) commit ([0-9]{1,12})\n', info)
    if not match or match[1].decode() != oid:
        raise RuntimeError('Raw commit object unavailable or wrong type')
    size = int(match[2])
    if not 0 < size <= min(MAX_COMMIT_BYTES, byte_budget):
        raise RuntimeError('Raw commit byte limit exceeded')
    raw = _bounded_command(command + ['commit', oid], env, size, deadline-time.monotonic())
    if len(raw) != size:
        raise RuntimeError('Raw commit short read')
    return parse_commit(oid, raw)


def witness(source, head, anchor, env):
    if not HEX.fullmatch(head) or not HEX.fullmatch(anchor):
        raise RuntimeError('Repository ancestry identity refused')
    deadline = time.monotonic() + MAX_GRAPH_SECONDS
    queue = [head]
    discovered = {head}
    child = {}
    objects = []
    total = 0
    while queue:
        if time.monotonic() >= deadline:
            raise RuntimeError('Repository ancestry deadline exceeded')
        oid = queue.pop()
        row = _raw_commit(source, oid, env, deadline, byte_budget=MAX_TOTAL_BYTES-total)
        objects.append(row)
        total += row['bytes']
        if total > MAX_TOTAL_BYTES or len(objects) > MAX_COMMITS:
            raise RuntimeError('Repository ancestry limit exceeded')
        if oid == anchor:
            path = [anchor]
            while path[-1] != head:
                path.append(child[path[-1]])
            return {'path':list(reversed(path)), 'objects':objects, 'bytes':total,
                    'limits':{'commits':MAX_COMMITS,'bytes':MAX_TOTAL_BYTES,
                              'commit_bytes':MAX_COMMIT_BYTES,'parents':MAX_PARENTS,
                              'seconds':MAX_GRAPH_SECONDS}}
        for parent in reversed(row['parents']):
            if parent not in discovered:
                discovered.add(parent)
                if len(discovered) > MAX_COMMITS:
                    raise RuntimeError('Repository ancestry graph limit exceeded')
                child[parent] = oid
                queue.append(parent)
    raise RuntimeError('Signed source lacks authenticated repository ancestry')


def admit(source, head, tree, env):
    anchor = observe()
    proof = witness(source, head, anchor['sha'], env)
    if proof['objects'][0]['tree'] != tree:
        raise RuntimeError('Repository candidate tree differs from raw HEAD')
    return {'schema':1, 'kind':'infra_repository_lineage_v1', 'head':head, 'tree':tree,
            'anchor':anchor, 'witness':proof}


def recheck(source, record, env):
    current = observe()
    record['dispatch_observation'] = current
    if current['sha'] != record['anchor']['sha']:
        raise RuntimeError('REPOSITORY_ANCHOR_MOVED: obtain current main before retry')
    deadline = time.monotonic() + 5
    actual = _bounded_command(['/usr/bin/git','-C',str(source),'rev-parse','HEAD','HEAD^{tree}'],
                              env, 82, deadline-time.monotonic()).decode().splitlines()
    if actual != [record['head'],record['tree']]:
        raise RuntimeError('Repository candidate changed before dispatch')
    # Rehash raw HEAD independently of revision metadata and graft graph views.
    if _raw_commit(source, record['head'], env, deadline)['tree'] != record['tree']:
        raise RuntimeError('Repository candidate changed before dispatch')


def bind_packet(record, run_id, bundle_sha, manifest_sha):
    record['packet'] = {'head':record['head'], 'tree':record['tree'], 'run_id':run_id,
                        'bundle_sha256':bundle_sha, 'manifest_sha256':manifest_sha}
    return record


def verify_packet(record, head, tree, run_id, bundle_sha, manifest_sha):
    expected = {'head':head, 'tree':tree, 'run_id':run_id,
                'bundle_sha256':bundle_sha, 'manifest_sha256':manifest_sha}
    if (record.get('head') != head or record.get('tree') != tree
            or record.get('packet') != expected or 'dispatch_observation' not in record
            or record['dispatch_observation']['sha'] != record['anchor']['sha']):
        raise RuntimeError('Repository admission packet binding differs')
