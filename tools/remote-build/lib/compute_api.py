"""Read-only gcloud credentials for Monitoring; no fleet provisioning dependency."""
import subprocess
from types import SimpleNamespace


def get_auth(project):
    token=subprocess.check_output(['gcloud','auth','print-access-token'],text=True,timeout=15).strip()
    if not token:
        raise RuntimeError('gcloud returned an empty access token')
    return SimpleNamespace(token=token,project=project)
