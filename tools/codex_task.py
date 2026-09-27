#!/usr/bin/env python3
"""Launch a scoped Codex task with explicit routing and a durable receipt."""
from __future__ import annotations

import argparse
from datetime import datetime, timezone
import hashlib
import json
import os
from pathlib import Path
import signal
import subprocess
import sys
import time
import uuid

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / 'scripts'))
from ops_paths import durable_root, ops_root

ROUTES = {
    'inventory': ('gpt-6-luna', 'low'),
    'implementation': ('gpt-6-sol', 'medium'),
    'parser': ('gpt-6-sol', 'high'),
    'review': ('gpt-6-sol', 'medium'),
    'acceptance': ('gpt-6-astra', 'high'),
}
REVIEW_ROLES = {'review', 'acceptance'}


def git(root, *args):
    return subprocess.check_output(['git', '-C', str(root), *args], text=True).strip()


def snapshot(root):
    return {'head': git(root, 'rev-parse', 'HEAD'),
            'tree': git(root, 'rev-parse', 'HEAD^{tree}'),
            'dirty': git(root, 'status', '--porcelain', '--untracked-files=all')}


def write_receipt(path, value):
    pending = path.with_suffix('.tmp')
    pending.write_text(json.dumps(value, indent=2) + '\n')
    pending.replace(path)


def usage_from(path):
    total = {}
    for line in path.read_text(errors='replace').splitlines():
        try:
            event = json.loads(line)
        except ValueError:
            continue
        if event.get('type') == 'turn.completed':
            for key, value in (event.get('usage') or {}).items():
                if isinstance(value, int):
                    total[key] = total.get(key, 0) + value
    # Some Codex review versions emit all-zero usage despite running a model.
    return total if any(total.values()) else None


def stop_process_group(process):
    """Stop this launcher's child and any subprocesses still in its session."""
    try:
        os.killpg(process.pid, signal.SIGTERM)
    except ProcessLookupError:
        process.wait()
        return
    try:
        process.wait(timeout=5)
    except subprocess.TimeoutExpired:
        pass
    finally:
        # The leader may exit before its descendants. Reap it and stop any
        # remaining group members even when the leader has already finished.
        try:
            os.killpg(process.pid, signal.SIGKILL)
        except ProcessLookupError:
            pass
        process.wait()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('role', choices=ROUTES)
    parser.add_argument('--cwd', type=Path, default=Path.cwd())
    parser.add_argument('--base', help='Full review base, or last reviewed head for a delta')
    parser.add_argument('--brief', type=Path, help='Small task or review-focus brief')
    parser.add_argument('--model', choices=['gpt-6-luna', 'gpt-6-sol', 'gpt-6-astra'])
    parser.add_argument('--effort', choices=['low', 'medium', 'high', 'xhigh', 'max'])
    parser.add_argument('--reason', help='Reason for any routing override')
    parser.add_argument('--output-dir', type=Path, help='New evidence directory outside the checkout')
    parser.add_argument('--timeout', type=int, default=1800, help='Maximum seconds for this task')
    parser.add_argument('--dry-run', action='store_true')
    args = parser.parse_args()
    review = args.role in REVIEW_ROLES
    if (args.model or args.effort) and not (args.reason or '').strip():
        parser.error('a model or effort override requires --reason')
    if review and not args.base:
        parser.error('review requires --base')
    if not review and (not args.brief or args.base):
        parser.error('non-review tasks require --brief and do not accept --base')
    if args.timeout <= 0:
        parser.error('--timeout must be positive')
    root = Path(git(args.cwd.resolve(), 'rev-parse', '--show-toplevel'))
    state = snapshot(root)
    if review and state['dirty']:
        parser.error('commit the candidate locally first: review requires a clean checkout')
    base = git(root, 'rev-parse', '--verify', args.base + '^{commit}') if review else None
    if review and subprocess.run(['git', '-C', str(root), 'merge-base', '--is-ancestor',
                                  base, state['head']], capture_output=True).returncode:
        parser.error('review base must be an ancestor of the candidate')
    brief = args.brief.read_text() if args.brief else ''
    model, effort = ROUTES[args.role]
    model, effort = args.model or model, args.effort or effort
    stamp = datetime.now(timezone.utc).strftime('%Y%m%dT%H%M%SZ')
    try:
        candidate = args.output_dir or (
            ops_root() / 'evidence/codex' / f'{stamp}-{args.role}-{uuid.uuid4().hex[:8]}')
        output = durable_root(candidate.expanduser(), 'review evidence directory')
    except ValueError as error:
        parser.error(str(error))
    if output.is_relative_to(root):
        parser.error('evidence must be outside the reviewed checkout')
    if output.exists():
        parser.error('output directory already exists; keep prior evidence intact')
    command = ['codex', 'exec', '--ignore-user-config', '--ephemeral', '--model', model,
               '-c', f'model_reasoning_effort={json.dumps(effort)}',
               '-c', 'features.plugins=false', '-c', 'features.hooks=false',
               '--sandbox', 'read-only' if review or args.role == 'inventory' else 'workspace-write',
               '--json', '--output-last-message', str(output / 'result.md')]
    if review:
        command += ['-c', f'review_model={json.dumps(model)}']
        if brief:
            command += ['-c', 'developer_instructions=' + json.dumps(brief)]
        command += ['review', '--base', base]
    else:
        command += ['-']
    receipt = {'role': args.role, 'model': model, 'effort': effort, 'reason': args.reason,
               'cwd': str(root), 'base': base, **state, 'started': stamp,
               'brief_sha256': hashlib.sha256(brief.encode()).hexdigest(),
               'command': command, 'status': 'running', 'approval': 'not_assessed'}
    if args.dry_run:
        print(json.dumps(receipt, indent=2))
        return 0
    output.mkdir(parents=True)
    record = output / 'receipt.json'
    write_receipt(record, receipt)
    code = 1
    interrupted = None

    def request_stop(signum, _frame):
        nonlocal interrupted
        # Set a flag instead of raising between Popen returning and assigning
        # its handle. The supervisor then always owns and cleans up the child.
        interrupted = signum

    handlers = {sig: signal.signal(sig, request_stop)
                for sig in (signal.SIGTERM, signal.SIGHUP, signal.SIGINT)}
    try:
        with (output/'events.jsonl').open('w') as events, (output/'stderr.log').open('w') as errors:
            process = subprocess.Popen(command, cwd=root, stdin=subprocess.PIPE, stdout=events,
                                       stderr=errors, text=True, start_new_session=True)
            try:
                deadline = time.monotonic() + args.timeout
                task_input = None if review else brief
                while True:
                    if interrupted:
                        code = 128 + interrupted
                        break
                    remaining = deadline - time.monotonic()
                    if remaining <= 0:
                        code = 124
                        break
                    try:
                        process.communicate(task_input, timeout=min(1, remaining))
                        code = process.returncode
                        break
                    except subprocess.TimeoutExpired:
                        task_input = None
            finally:
                stop_process_group(process)
        receipt['status'] = 'completed' if code == 0 else 'failed'
        receipt['usage'] = usage_from(output/'events.jsonl')
        receipt['after'] = snapshot(root)
        if review and receipt['after'] != state:
            receipt['status'], code = 'stale', 2
        if code == 0 and not (output/'result.md').is_file():
            receipt['status'], code = 'missing_result', 2
    except (OSError, subprocess.SubprocessError) as error:
        code = 1
        receipt['status'] = 'failed'
        receipt['error'] = str(error)
    finally:
        if interrupted:
            receipt['status'], code = 'interrupted', 128 + interrupted
        receipt['exit_code'] = code
        receipt['finished'] = datetime.now(timezone.utc).isoformat()
        write_receipt(record, receipt)
        for sig, handler in handlers.items():
            signal.signal(sig, handler)
    print(record)
    return code


if __name__ == '__main__':
    sys.exit(main())
