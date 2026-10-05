"""Select an idle remote-builder host using complete Monitoring windows."""
import json
import math
import shlex
import subprocess
from types import SimpleNamespace
from .config import approved_instances, builder_instance_name, matching_approval
from .resource_metrics import read_utilization

# Exact launcher bytes provisioned by spot-github-runners' builder_assets.py.
# The launcher has no version verb, so update this digest with its protocol.
LAUNCHER_SHA256 = 'd3781dd0d320477ac7201a1e9b94ed2b59b829efb88b38b794ae93a467d0d2f4'
SSH_KEEPALIVE = ('--ssh-flag=-oServerAliveInterval=15',
                 '--ssh-flag=-oServerAliveCountMax=3')


def launcher_probe(launcher='/usr/local/bin/oxidex-remote-build',
                   config='/etc/oxidex-remote-build.json',
                   drain='/run/oxidex-build-draining',
                   digest=LAUNCHER_SHA256, sudo='sudo '):
    """Check the exact launcher and execute its side-effect-free protocol path."""
    executable = shlex.quote(launcher)
    return (f'test -f {shlex.quote(config)} && '
            f'test ! -e {shlex.quote(drain)} && '
            f'test -x {executable} && '
            f'test "$(sha256sum {executable} | cut -d" " -f1)" = {shlex.quote(digest)} && '
            f'output=$({sudo}{executable} invalid.project prepare 2>&1); '
            'code=$?; test "$code" -eq 1 && '
            'case "$output" in *"Project must be a simple identifier, never a path"*) true;; *) false;; esac')


def rank_workers(rows):
    eligible = [(vm, sample) for vm, sample in rows if sample is not None
                and len(sample) == 2 and all(math.isfinite(v) and 0 <= v < .75 for v in sample)]
    def capacity(row):
        vm, sample = row
        cpu=getattr(vm,'cpus',1)*(1-sample[0])
        ram=getattr(vm,'memory_gib',4)*(1-sample[1])
        # Balanced capacity: four GiB per available core; break ties by CPU then RAM.
        return (-min(cpu,ram/4),-cpu,-ram,vm.name)
    return sorted(eligible, key=capacity)


def select_worker(project, excluded_ids=()):
    inventory = json.loads(subprocess.check_output([
        'gcloud', 'compute', 'instances', 'list', '--project='+project,
        '--format=json(name,id,zone,status,machineType)'], text=True))
    candidates = []
    approvals = approved_instances()
    for row in inventory:
        zone = row['zone'].rsplit('/',1)[-1]
        permitted = builder_instance_name(row.get('name')) or matching_approval(
            approvals, project, row.get('name'), zone, row.get('id'))
        if str(row.get('id')) in excluded_ids or row.get('status') != 'RUNNING' or not permitted:
            continue
        vm = SimpleNamespace(name=row['name'], instance_id=str(row['id']), zone=row['zone'].rsplit('/',1)[-1])
        if row.get('machineType'):
            machine=row['machineType'].rsplit('/',1)[-1]
            spec=json.loads(subprocess.check_output(['gcloud','compute','machine-types','describe',machine,
                '--zone='+vm.zone,'--project='+project,'--format=json(guestCpus,memoryMb)'],text=True))
            vm.cpus=spec['guestCpus'];vm.memory_gib=spec['memoryMb']/1024
        candidates.append(vm)
    samples=read_utilization(project,candidates) if candidates else None
    observations=list(zip(candidates,samples)) if samples else [(vm,None) for vm in candidates]
    # Probe only candidates with trustworthy low utilization, best first.
    probe = launcher_probe()
    for vm, sample in rank_workers(observations):
        try:
            result = subprocess.run(['gcloud','compute','ssh',vm.name,'--zone='+vm.zone,
            '--project='+project,'--quiet',*SSH_KEEPALIVE,
            '--command=sh -c '+shlex.quote(probe)],
                stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, timeout=60)
        except subprocess.TimeoutExpired:
            continue
        if result.returncode == 0:
            print(f'Selected {vm.name} ({vm.zone}): CPU {sample[0]:.1%}, memory {sample[1]:.1%}', flush=True)
            return vm, sample
    raise RuntimeError('No available builder-* or explicitly approved VM with complete CPU/memory metrics below 75%; retry later')
