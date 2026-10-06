"""Explicit uploader authentication shared by SSH and artifact transfers."""
import os
import ipaddress
import json
import subprocess
import re
from pathlib import Path


def identity(environ=None):
    environ = os.environ if environ is None else environ
    user = environ.get('OXIDEX_REMOTE_SSH_USER')
    key = environ.get('OXIDEX_REMOTE_SSH_KEY')
    if not user and not key:
        return None
    if not user or not key or not re.fullmatch(r'[a-z_][a-z0-9_-]{0,31}', user):
        raise ValueError('Configure both a valid OXIDEX_REMOTE_SSH_USER and OXIDEX_REMOTE_SSH_KEY')
    path = Path(key).expanduser()
    if not path.is_absolute() or not path.is_file() or not os.access(path, os.R_OK):
        raise ValueError('Explicit remote SSH key must be an absolute readable file')
    return user, str(path)


def target(instance):
    value = identity()
    return value[0] + '@' + instance if value else instance


def flags(kind):
    value = identity()
    if not value:
        return []
    if kind not in ('ssh', 'scp'):
        raise ValueError('Unknown SSH transport')
    prefix = '--' + kind + '-flag='
    return ['--ssh-key-file=' + value[1], prefix + '-F/dev/null',
            prefix + '-oIdentityAgent=none', prefix + '-oIdentitiesOnly=yes']


class DirectTransport:
    """Use pre-enrolled access; never let gcloud change VM SSH metadata."""

    def __init__(self, instance, zone, project, instance_id=None):
        value = identity()
        if value is None:
            raise ValueError('Direct transport requires an explicit uploader identity')
        known = Path(os.environ.get('OXIDEX_REMOTE_SSH_KNOWN_HOSTS', '')).expanduser()
        if not os.environ.get('OXIDEX_REMOTE_SSH_KNOWN_HOSTS') or not known.is_absolute() or not known.is_file():
            raise ValueError('Configure an absolute trusted OXIDEX_REMOTE_SSH_KNOWN_HOSTS file')
        actual = json.loads(subprocess.check_output([
            'gcloud', 'compute', 'instances', 'describe', instance,
            '--zone='+zone, '--project='+project,
            '--format=json(id,status,networkInterfaces)'], text=True, timeout=30))
        actual_id = str(actual.get('id', ''))
        if (not re.fullmatch(r'[0-9]{1,20}', actual_id)
                or instance_id is not None and actual_id != str(instance_id)
                or actual.get('status') != 'RUNNING'):
            raise RuntimeError('Direct SSH provider identity changed or is not running')
        addresses = [a.get('natIP') for n in actual.get('networkInterfaces', [])
                     for a in n.get('accessConfigs', []) if a.get('natIP')]
        if len(addresses) != 1:
            raise RuntimeError('Direct SSH requires exactly one provider public address')
        self.host = str(ipaddress.ip_address(addresses[0]))
        # No keyscan, automatic trust, metadata writes, or ambient SSH config.
        self.options = ['-F', '/dev/null', '-i', value[1],
                        '-o', 'IdentityAgent=none', '-o', 'IdentitiesOnly=yes',
                        '-o', 'BatchMode=yes', '-o', 'StrictHostKeyChecking=yes',
                        '-o', 'UserKnownHostsFile='+str(known),
                        '-o', 'GlobalKnownHostsFile=/dev/null',
                        '-o', 'ConnectTimeout=15',
                        '-o', 'ServerAliveInterval=15', '-o', 'ServerAliveCountMax=3']
        self.destination = value[0]+'@'+self.host
        self.instance_id = actual_id

    def ssh(self, command):
        return ['ssh', *self.options, self.destination, command]

    def scp(self, local, remote, download=False):
        host = '['+self.host+']' if ':' in self.host else self.host
        destination = self.destination.split('@')[0]+'@'+host+':'+remote
        paths = [destination, str(local)] if download else [str(local), destination]
        return ['scp', *self.options, *paths]
