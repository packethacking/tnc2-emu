#!/usr/bin/env python3
"""Drive a TheNet console over TCP: log in from the RS232 port and run commands.

usage: console.py HOST PORT [command ...]
Each command is sent with a CR, then output is read until the prompt is
quiet. The raw session is printed to stdout (CR shown as newline).
"""
import socket
import sys
import time


def read_for(s, seconds):
    s.settimeout(0.2)
    out = b""
    end = time.time() + seconds
    while time.time() < end:
        try:
            d = s.recv(4096)
            if not d:
                break
            out += d
            end = max(end, time.time() + 0.8)
        except socket.timeout:
            pass
    return out


def show(b):
    sys.stdout.write(b.decode("latin-1").replace("\r\n", "\n").replace("\r", "\n"))
    sys.stdout.flush()


def main():
    host, port = sys.argv[1], int(sys.argv[2])
    cmds = sys.argv[3:]
    s = socket.create_connection((host, port))
    s.sendall(b"\x1b")
    show(read_for(s, 1.5))
    s.sendall(b"C\r")
    show(read_for(s, 2))
    for c in cmds:
        if c.startswith("sleep:"):
            time.sleep(float(c[6:]))
            continue
        wait = 3
        if c.startswith("wait") and ":" in c:
            w, c = c.split(":", 1)
            wait = float(w[4:])
        s.sendall(c.encode() + b"\r")
        show(read_for(s, wait))
    s.close()


if __name__ == "__main__":
    main()
