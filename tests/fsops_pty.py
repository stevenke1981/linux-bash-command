"""Exercise interactive filesystem prompts entirely inside a temporary directory."""
import errno
import os
from pathlib import Path
import pty
import select
import subprocess
import sys
import tempfile
import time


def interact(binary_dir, sandbox, name, arguments, reply, expected=0):
    master, slave = pty.openpty()
    child = subprocess.Popen(
        [str(binary_dir / name), *arguments], cwd=sandbox,
        stdin=slave, stdout=slave, stderr=slave,
        env={**os.environ, "LC_ALL": "C", "TZ": "UTC", "AGENTCMD_COLOR": "never"},
    )
    os.close(slave)
    output = bytearray()
    sent = False
    deadline = time.monotonic() + 10
    try:
        while time.monotonic() < deadline:
            ready, _, _ = select.select([master], [], [], 0.1)
            if ready:
                try:
                    block = os.read(master, 65536)
                except OSError as error:
                    if error.errno == errno.EIO:
                        break
                    raise
                if not block:
                    break
                output.extend(block)
                if not sent and (b"?" in output or b"<" in output):
                    os.write(master, reply + b"\n")
                    sent = True
            elif child.poll() is not None:
                break
        assert child.wait(timeout=1) == expected, (name, bytes(output))
        assert sent, (name, "no interactive prompt", bytes(output))
        return bytes(output)
    finally:
        if child.poll() is None:
            child.kill()
        child.wait()
        os.close(master)


def main():
    binary_dir = Path(sys.argv[1]).resolve()
    with tempfile.TemporaryDirectory(prefix="agentcmd-fs-pty-") as directory:
        sandbox = Path(directory)
        (sandbox / "source").write_bytes(b"new")
        (sandbox / "destination").write_bytes(b"old")
        interact(binary_dir, sandbox, "cp", ["-i", "source", "destination"], b"n", expected=1)
        assert (sandbox / "destination").read_bytes() == b"old"
        interact(binary_dir, sandbox, "cp", ["-i", "source", "destination"], b"y")
        assert (sandbox / "destination").read_bytes() == b"new"
        interact(binary_dir, sandbox, "rm", ["-i", "destination"], b"n")
        assert (sandbox / "destination").exists()
        interact(binary_dir, sandbox, "rm", ["-i", "destination"], b"y")
        assert not (sandbox / "destination").exists()
        output = interact(
            binary_dir, sandbox, "find", ["source", "-ok", "echo", "{}", ";"], b"y"
        )
        assert b"source" in output
    print("Filesystem PTY prompts passed")


if __name__ == "__main__":
    main()
