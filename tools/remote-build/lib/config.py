"""Read Cargo's string and structured environment settings consistently."""

import re
import json
from pathlib import Path
import sys
sys.path.insert(0, str(Path(__file__).resolve().parents[3]/'scripts'))
from ops_paths import ops_root


def builder_instance_name(name):
    """Require the dedicated builder VM role before any remote build command."""
    return isinstance(name, str) and len(name) <= 63 and re.fullmatch(
        r'builder-[a-z0-9](?:[a-z0-9-]*[a-z0-9])?', name) is not None


def approved_instances():
    """Read explicit, identity-bound exceptions; disabled entries are staging only."""
    path = ops_root()/'config'/'remote-build.json'
    if not path.exists():
        return []
    data = json.loads(path.read_text())
    if not isinstance(data, dict) or data.get('schema_version') != 1 or not isinstance(data.get('instances'), list):
        raise ValueError(f'Invalid remote builder approval configuration: {path}')
    result = []
    seen = set()
    for row in data['instances']:
        if (not isinstance(row, dict)
                or set(row) != {'name', 'id', 'project', 'zone', 'enabled'}
                or any(not isinstance(row[k], str) or not re.fullmatch('[a-z0-9-]+', row[k])
                       for k in ('name', 'project', 'zone'))
                or not isinstance(row['id'], str) or not re.fullmatch('[0-9]{1,20}', row['id'])
                or not isinstance(row['enabled'], bool)):
            raise ValueError(f'Invalid identity-bound remote builder approval: {path}')
        key = (row['project'], row['zone'], row['name'])
        if key in seen:
            raise ValueError(f'Duplicate remote builder approval: {path}')
        seen.add(key)
        if row['enabled']:
            result.append(row)
    return result


def matching_approval(rows, project, name, zone, instance_id=None):
    return next((row for row in rows if row['project'] == project and row['name'] == name
                 and row['zone'] == zone
                 and (instance_id is None or row['id'] == str(instance_id))), None)


def cargo_env_value(settings, name):
    value = settings.get(name)
    if isinstance(value, dict):
        if 'value' not in value:
            raise ValueError(f'{name} in .cargo/config.toml must have a string value')
        value = value['value']
    if value is not None and not isinstance(value, str):
        raise ValueError(f'{name} in .cargo/config.toml must have a string value')
    return value
