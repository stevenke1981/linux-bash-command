#!/usr/bin/env python3
"""Linux-only gzip terminal and overwrite checks, confined to a sandbox."""
import os
import pty
import subprocess
import sys
import tempfile
import termios
from pathlib import Path

exe = os.path.abspath(sys.argv[1])
with tempfile.TemporaryDirectory(prefix="agentcmd-gzip-pty-") as root:
    path = Path(root)
    (path / "f").write_bytes(b"input\n")
    (path / "f.gz").write_bytes(b"existing")
    master, slave = pty.openpty()
    child = subprocess.Popen([exe, "f"], cwd=root, stdin=slave, stdout=subprocess.PIPE, stderr=subprocess.PIPE)
    os.close(slave)
    os.write(master, b"n\n")
    _, stderr = child.communicate(timeout=10)
    os.close(master)
    assert child.returncode != 0, (child.returncode, stderr)
    assert (path / "f").read_bytes() == b"input\n"
    assert (path / "f.gz").read_bytes() == b"existing"
    assert b"overwrite" in stderr.lower(), stderr

    master, slave = pty.openpty()
    attrs = termios.tcgetattr(slave)
    attrs[1] &= ~termios.OPOST
    termios.tcsetattr(slave, termios.TCSANOW, attrs)
    child = subprocess.Popen([exe, "-c"], cwd=root, stdin=subprocess.PIPE, stdout=slave, stderr=subprocess.PIPE)
    os.close(slave)
    _, stderr = child.communicate(input=b"input\n", timeout=10)
    os.close(master)
    assert child.returncode != 0, (child.returncode, stderr)
    assert b"terminal" in stderr.lower(), stderr

    master, slave = pty.openpty()
    attrs = termios.tcgetattr(slave)
    attrs[1] &= ~termios.OPOST
    termios.tcsetattr(slave, termios.TCSANOW, attrs)
    child = subprocess.Popen([exe, "-fc"], cwd=root, stdin=subprocess.PIPE, stdout=slave, stderr=subprocess.PIPE)
    os.close(slave)
    _, stderr = child.communicate(input=b"input\n", timeout=10)
    data = os.read(master, 4096)
    os.close(master)
    assert child.returncode == 0, (child.returncode, stderr)
    assert data.startswith(b"\x1f\x8b"), data
print("gzip terminal checks passed")
