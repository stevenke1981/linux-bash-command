import fcntl
import os
import pty
import select
import signal
import struct
import sys
import termios
import time

binary, source = sys.argv[1:]
pid, master = pty.fork()
if pid == 0:
    os.execv(binary, [binary, source])
fcntl.ioctl(master, termios.TIOCSWINSZ, struct.pack("HHHH", 24, 80, 0, 0))
output = bytearray()
deadline = time.monotonic() + 10
try:
    while b"-- More --" not in output and time.monotonic() < deadline:
        ready, _, _ = select.select([master], [], [], 1)
        if ready:
            output.extend(os.read(master, 4096))
    if b"-- More --" not in output:
        raise AssertionError("pager prompt did not appear")
    os.write(master, b"q")
    while time.monotonic() < deadline:
        exited, status = os.waitpid(pid, os.WNOHANG)
        if exited:
            if not os.WIFEXITED(status) or os.WEXITSTATUS(status) != 0:
                raise AssertionError(f"pager exited abnormally: {status}")
            print("interactive less passed")
            break
        time.sleep(0.1)
    else:
        raise AssertionError("pager did not exit after q")
except BaseException:
    os.kill(pid, signal.SIGKILL)
    os.waitpid(pid, 0)
    raise
finally:
    os.close(master)
