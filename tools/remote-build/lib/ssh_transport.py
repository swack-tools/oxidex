"""Explicit uploader authentication shared by SSH and artifact transfers."""
import os
import re
from pathlib import Path


def identity():
    user = os.environ.get('OXIDEX_REMOTE_SSH_USER')
    key = os.environ.get('OXIDEX_REMOTE_SSH_KEY')
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
