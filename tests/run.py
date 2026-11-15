# SPDX-License-Identifier: BSD-3-Clause

import os
import re
import shutil
import subprocess
import sys
import tempfile

NOISE_RE = re.compile(r"\x1b\[[0-9;]*m|\r|^\[[ \d.]+\] (?:syslog\(\w+\): )?")
PLAN_RE = re.compile(r"^1\.\.\d+")
TAP_RE = re.compile(r"^(?:ok|not ok|#|1\.\.\d+|Bail out!)")

sep = sys.argv.index("--")
with tempfile.TemporaryDirectory(prefix="zinnia-esp-") as esp:
    for spec in sys.argv[1:sep]:
        src, dest = spec.split("=", 1)
        path = os.path.join(esp, dest)
        os.makedirs(os.path.dirname(path), exist_ok=True)
        shutil.copyfile(src, path)

    proc = subprocess.Popen(
        sys.argv[sep + 1 :]
        + ["-drive", f"format=raw,file=fat:{esp},if=virtio,readonly=on"],
        stdin=subprocess.DEVNULL,
        stdout=subprocess.PIPE,
        text=True,
        errors="replace",
    )
    for line in proc.stdout:
        line = NOISE_RE.sub("", line).rstrip()
        print(line if TAP_RE.match(line) else f"# {line}", flush=True)
        if PLAN_RE.match(line):
            proc.kill()
            proc.wait()
            sys.exit(0)
    proc.wait()
sys.exit(1)
