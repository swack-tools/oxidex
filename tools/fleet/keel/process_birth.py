"""Kernel process birth tokens for distinguishing a reused worker PGID.

Command lines can disappear on exec, and a PID alone is reusable. A missing
or unreadable token is never evidence that the old worker died.
"""

from __future__ import annotations

import ctypes
import os
import re
import sys
from pathlib import Path
from typing import Optional


def valid_birth_token(value: object) -> bool:
    """Reject malformed durable evidence rather than call it replacement."""
    if not isinstance(value, str):
        return False
    if re.fullmatch(r"darwin:[1-9][0-9]*:[0-9]{1,6}", value):
        return int(value.rsplit(":", 1)[1]) < 1_000_000
    return bool(re.fullmatch(
        r"linux:[0-9a-fA-F]{8}-(?:[0-9a-fA-F]{4}-){3}[0-9a-fA-F]{12}:[1-9][0-9]*",
        value))


class _ProcBSDInfo(ctypes.Structure):
    # Darwin SDK sys/proc_info.h, struct proc_bsdinfo (MAXCOMLEN = 16).
    _fields_ = [
        ("flags", ctypes.c_uint32), ("status", ctypes.c_uint32),
        ("xstatus", ctypes.c_uint32), ("pid", ctypes.c_uint32),
        ("ppid", ctypes.c_uint32), ("uid", ctypes.c_uint32),
        ("gid", ctypes.c_uint32), ("ruid", ctypes.c_uint32),
        ("rgid", ctypes.c_uint32), ("svuid", ctypes.c_uint32),
        ("svgid", ctypes.c_uint32), ("reserved", ctypes.c_uint32),
        ("comm", ctypes.c_char * 16), ("name", ctypes.c_char * 32),
        ("nfiles", ctypes.c_uint32), ("pgid", ctypes.c_uint32),
        ("pjobc", ctypes.c_uint32), ("tdev", ctypes.c_uint32),
        ("tpgid", ctypes.c_uint32), ("nice", ctypes.c_int32),
        ("start_sec", ctypes.c_uint64), ("start_usec", ctypes.c_uint64),
    ]


def leader_birth(pid: int) -> Optional[str]:
    """Return a kernel birth token only for a same-UID group leader."""
    if not isinstance(pid, int) or pid <= 1:
        return None
    if sys.platform == "darwin":
        try:
            lib = ctypes.CDLL("/usr/lib/libproc.dylib", use_errno=True)
            info = _ProcBSDInfo()
            lib.proc_pidinfo.argtypes = [ctypes.c_int, ctypes.c_int,
                                         ctypes.c_uint64, ctypes.c_void_p,
                                         ctypes.c_int]
            lib.proc_pidinfo.restype = ctypes.c_int
            size = ctypes.sizeof(info)
            got = lib.proc_pidinfo(pid, 3, 0, ctypes.byref(info), size)
            if (got != size or info.pid != pid or info.pgid != pid or
                    info.uid != os.getuid() or info.start_sec == 0 or
                    info.start_usec >= 1_000_000):
                return None
            return f"darwin:{info.start_sec}:{info.start_usec}"
        except (OSError, AttributeError, ValueError):
            return None
    if sys.platform.startswith("linux"):
        try:
            raw = (Path("/proc") / str(pid) / "stat").read_text()
            rest = raw[raw.rindex(")") + 2:].split()
            # Fields 3.. start here. Field 5 is pgrp, 22 is starttime.
            if (int(raw.split("(", 1)[0].strip()) != pid or
                    int(rest[2]) != pid or rest[0] == "Z" or
                    os.stat(Path("/proc") / str(pid)).st_uid != os.getuid()):
                return None
            tick = int(rest[19])
            boot = Path("/proc/sys/kernel/random/boot_id").read_text().strip()
            if tick <= 0 or not boot:
                return None
            return f"linux:{boot}:{tick}"
        except (OSError, ValueError, IndexError):
            return None
    return None
