"""Read complete identity-scoped five-minute CPU and memory windows."""
import json
import logging
import math
import time
import urllib.request
from datetime import datetime, timezone
from urllib.parse import urlencode
from .compute_api import get_auth

CPU = 'compute.googleapis.com/instance/cpu/utilization'
MEMORY = 'agent.googleapis.com/memory/percent_used'
LOG = logging.getLogger('spot_runner_autoscaler')


def instant(value):
    return datetime.fromisoformat(value.replace('Z', '+00:00')).timestamp()


def normalize(rows, identities, now):
    end = now - 120
    buckets = {}
    try:
        for row in rows:
            labels = row['resource']['labels']
            identity = (labels['instance_id'], labels['project_id'], labels['zone'])
            metric = row['metric']['type']
            if metric not in (CPU, MEMORY):
                continue
            if metric == MEMORY and row['metric'].get('labels', {}).get('state') != 'used':
                continue
            points = buckets.setdefault((identity, metric), {})
            for point in row.get('points', []):
                timestamp = instant(point['interval']['endTime'])
                if not end - 300 <= timestamp <= end:
                    continue
                value = float(point['value']['doubleValue']) / (100 if metric == MEMORY else 1)
                if not math.isfinite(value) or not 0 <= value <= 1:
                    return None
                if timestamp in points and points[timestamp] != value:
                    return None
                points[timestamp] = value
        samples = []
        for identity in identities:
            pair = []
            for metric in (CPU, MEMORY):
                values = buckets.get((identity, metric), {})
                if len(values) < 4 or now - max(values) > 300:
                    break
                pair.append(sum(values.values()) / len(values) if metric == CPU else max(values.values()))
            samples.append(tuple(pair) if len(pair)==2 else None)
        return samples or None
    except (KeyError, TypeError, ValueError, OverflowError):
        return None


def read_utilization(project, vms, timestamp=None):
    timestamp = time.time() if timestamp is None else timestamp
    if not vms or any(not vm.instance_id for vm in vms):
        return None
    auth = get_auth(project)
    end = datetime.fromtimestamp(timestamp-120, timezone.utc).isoformat()
    start = datetime.fromtimestamp(timestamp-420, timezone.utc).isoformat()
    rows = []
    for metric in (CPU, MEMORY):
        query = {'filter': f'metric.type = "{metric}" AND resource.type = "gce_instance"',
                 'interval.startTime': start, 'interval.endTime': end, 'view': 'FULL', 'pageSize': '1000'}
        identifiers=' OR '.join(f'resource.labels.instance_id = "{vm.instance_id}"' for vm in vms)
        query['filter'] += ' AND ('+identifiers+')'
        if metric == MEMORY:
            query['filter'] += ' AND metric.labels.state = "used"'
        seen = set()
        while True:
            url = f'https://monitoring.googleapis.com/v3/projects/{project}/timeSeries?' + urlencode(query)
            request = urllib.request.Request(url, headers={'Authorization': f'Bearer {auth.token}'})
            with urllib.request.urlopen(request, timeout=30) as response:
                page = json.load(response)
            rows.extend(page.get('timeSeries', []))
            token = page.get('nextPageToken')
            if not token:
                break
            if token in seen:
                raise RuntimeError('Monitoring repeated a pagination token')
            seen.add(token)
            query['pageToken'] = token
    return normalize(rows, [(vm.instance_id, project, vm.zone) for vm in vms], timestamp)
