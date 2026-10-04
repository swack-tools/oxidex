"""Read Cargo's string and structured environment settings consistently."""

import re


def builder_instance_name(name):
    """Require the dedicated builder VM role before any remote build command."""
    return isinstance(name, str) and len(name) <= 63 and re.fullmatch(
        r'builder-[a-z0-9](?:[a-z0-9-]*[a-z0-9])?', name) is not None


def cargo_env_value(settings, name):
    value = settings.get(name)
    if isinstance(value, dict):
        if 'value' not in value:
            raise ValueError(f'{name} in .cargo/config.toml must have a string value')
        value = value['value']
    if value is not None and not isinstance(value, str):
        raise ValueError(f'{name} in .cargo/config.toml must have a string value')
    return value
