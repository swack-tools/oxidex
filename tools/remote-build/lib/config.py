"""Read Cargo's string and structured environment settings consistently."""


def cargo_env_value(settings, name):
    value = settings.get(name)
    if isinstance(value, dict):
        if 'value' not in value:
            raise ValueError(f'{name} in .cargo/config.toml must have a string value')
        value = value['value']
    if value is not None and not isinstance(value, str):
        raise ValueError(f'{name} in .cargo/config.toml must have a string value')
    return value
