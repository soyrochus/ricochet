#!/usr/bin/env python3
"""Synthetic protocol/lifecycle checks. This is NOT a Ghostty emulator or visual QA.

Build the release binary and dev examples first; see README.md. Uses only Python's
standard library. No game input, terminal modes or resources escape the child PTY.
"""
import argparse
import base64
import zlib
import errno
import fcntl
import json
import os
from pathlib import Path
import pty
import re
import select
import signal
import statistics
import struct
import termios
import tempfile
import time

ROOT = Path(__file__).resolve().parents[1]
APC = re.compile(rb"\x1b_G(.*?)\x1b\\", re.S)
PROBE_ID = 0x524943
COMPRESSION_PROBE_ID = PROBE_ID + 1


def size(fd, cols=100, rows=40, width=1000, height=800):
    fcntl.ioctl(fd, termios.TIOCSWINSZ, struct.pack("HHHH", rows, cols, width, height))


def key(code, kind=1, modifier=1):
    return f"\x1b[{code};{modifier}:{kind}u".encode()


def resources(data):
    aliases, images, placements = {}, set(), set()
    peak_images = peak_placements = next_id = 0
    uploads = 0
    compressed_uploads = 0
    pending = None
    sizes = {}
    for match in APC.finditer(data):
        control = match[1].split(b";", 1)[0]
        fields = dict(item.split(b"=", 1) for item in control.split(b",") if b"=" in item)
        action = fields.get(b"a")
        number = int(fields.get(b"I", b"0"))
        placement = int(fields.get(b"p", b"0"))
        if action == b"t":
            assert pending is None, "interleaved image transmission"
            pending = [int(fields[b"s"]), int(fields[b"v"]), fields.get(b"o")==b"z", bytearray()]
            compressed_uploads += int(pending[2])
            sizes[number] = (pending[0],pending[1])
            next_id += 1
            uploads += 1
            aliases[number] = next_id
            images.add(next_id)
        elif action == b"p":
            assert number in aliases, ("placement before upload", number)
            placements.add((aliases[number], placement))
            if b"w" in fields:
                x,y,w,h=(int(fields[k]) for k in (b"x",b"y",b"w",b"h"))
                assert w>0 and h>0 and x+w<=sizes[number][0] and y+h<=sizes[number][1], ("source rectangle",fields)
        elif action == b"d" and fields.get(b"d") in (b"n", b"N"):
            ident = aliases.get(number)
            placements = {p for p in placements if not (p[0] == ident and (not placement or p[1] == placement))}
            if fields[b"d"] == b"N":
                images.discard(ident)
                aliases.pop(number, None)
        elif action == b"d":
            raise AssertionError(("unscoped deletion", control))
        if pending is not None:
            payload=match[1].split(b";",1)[1]
            assert len(payload)<=4096
            pending[3].extend(payload)
            if fields.get(b"m")==b"0":
                rgba=base64.b64decode(pending[3],validate=True)
                if pending[2]: rgba=zlib.decompress(rgba)
                assert len(rgba)==pending[0]*pending[1]*4, "RGBA size"
                pending=None
        peak_images = max(peak_images, len(images))
        peak_placements = max(peak_placements, len(placements))
    assert pending is None, "incomplete upload"
    assert not images and not placements, ("leaked model resources", images, placements)
    assert peak_images <= 16 and peak_placements <= 192, (peak_images, peak_placements)
    return dict(uploads=uploads, compressed_uploads=compressed_uploads, peak_images=peak_images, peak_placements=peak_placements)


def run_case(name, argv, actions=(), reply="supported", expect=0, text=None, runtime=3.0):
    pid, fd = pty.fork()
    if pid == 0:
        size(0)
        os.execv(str(ROOT / argv[0]), [str(ROOT / argv[0]), *argv[1:]])
    data = bytearray()
    began = time.monotonic()
    negotiated = None
    sent = set()
    frame_times = []
    scan_from = 0
    status = None
    eof = False
    try:
        while time.monotonic() - began < runtime:
            now = time.monotonic()
            if negotiated is not None:
                for index, (delay, action) in enumerate(actions):
                    if index not in sent and now - negotiated >= delay:
                        sent.add(index)
                        if callable(action):
                            action(pid, fd)
                        else:
                            os.write(fd, action)
            readable, _, _ = select.select([fd], [], [], 0.005)
            if readable:
                try:
                    chunk = os.read(fd, 131072)
                except OSError as error:
                    if error.errno != errno.EIO:
                        raise
                    chunk = b""
                if not chunk:
                    eof = True
                    break
                data.extend(chunk)
                # Keep enough overlap to recognize a split synchronization marker.
                while True:
                    index = data.find(b"\x1b[?2026h", scan_from)
                    if index < 0:
                        scan_from = max(scan_from, len(data) - 8)
                        break
                    frame_times.append(now)
                    scan_from = index + 9
                if negotiated is None and b"\x1b[c" in data:
                    negotiated = time.monotonic()
                    if reply != "silent":
                        graphics = f"\x1b_Gi={PROBE_ID};OK\x1b\\".encode() if reply != "no-graphics" else b""
                        compression = f"\x1b_Gi={COMPRESSION_PROBE_ID};{'EINVAL:unsupported' if reply == 'no-compression' else 'OK'}\x1b\\".encode() if reply != "missing-compression" else b""
                        flags = 3 if reply == "no-releases" else 11
                        # Include unrelated press/release input *among* the replies.
                        os.write(fd, graphics + compression + key(97) + key(97, 3) + f"\x1b[?{flags}u\x1b[?2026;2$y\x1b[?62;4c".encode())
            if status is None:
                done, child_status = os.waitpid(pid, os.WNOHANG)
                if done:
                    status = child_status
                # Continue draining output until the PTY closes.
        if status is None:
            done, child_status = os.waitpid(pid, 0 if eof else os.WNOHANG)
            if done:
                status = child_status
        if status is None:
            (Path(tempfile.gettempdir()) / f"ricochet-pty-{name}.bin").write_bytes(data)
            raise AssertionError(f"{name}: child did not exit within {runtime}s")
        code = os.waitstatus_to_exitcode(status)
        assert code == expect, (name, code, bytes(data[-1000:]))
        flags = termios.tcgetattr(fd)[3]
        assert flags & termios.ECHO and flags & termios.ICANON and flags & termios.ISIG, name
        for marker in [b"\x1b[?25h", b"\x1b[<1u", b"\x1b[?1004l", b"\x1b[?1049l"]:
            assert marker in data, (name, "missing cleanup", marker)
        assert data.count(b"\x1b[?2026h") <= data.count(b"\x1b[?2026l"), name
        if text:
            assert text.encode() in data, (name, text)
        if name.startswith("fault-"):
            assert data.find(b"\x1b[?1049l") < data.find(text.encode()), "error printed before restoration"
        resource_result=resources(data)
        if reply in ("no-compression", "missing-compression"): assert resource_result["compressed_uploads"]==0
        elif resource_result["uploads"]: assert resource_result["compressed_uploads"]==resource_result["uploads"]
        report = dict(case=name, exit=code, seconds=round(time.monotonic() - began, 3), **resource_result)
        # These are PTY output intervals, not actual terminal presentation timing.
        intervals = [(b-a)*1000 for a,b in zip(frame_times,frame_times[1:]) if b-a > 0.001]
        if intervals:
            report["pty_frame_interval_median_ms"] = round(statistics.median(intervals), 2)
        return report
    finally:
        if status is None:
            os.kill(pid, signal.SIGKILL)
            os.waitpid(pid, 0)
        os.close(fd)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output", type=Path)
    args = parser.parse_args()
    binary = ["target/release/ricochet"]
    cases = []
    cases.append(run_case("spike-key-releases", ["target/release/examples/terminal_spike"], [(0.1,key(100)),(0.25,key(100,3)),(0.4,key(113))],text="Release events: 2"))
    cases.append(run_case("gameplay-focus-pause-restart", binary, [(0.08,key(32)),(0.15,key(32)),(0.22,key(100)),(0.3,b"\x1b[O"),(0.36,b"\x1b[I"),(0.42,key(100,2)),(0.5,key(112)),(0.6,key(112)),(0.7,key(114)),(0.8,key(113))],text="PAUSED"))
    cases.append(run_case("resize-and-font-change", binary, [(0.08,key(32)),(0.14,key(32)),(0.25,lambda p,f:size(f,60,20,600,400)),(0.4,lambda p,f:size(f)),(0.55,key(112)),(0.65,lambda p,f:size(f,100,40,1200,960)),(0.8,key(113))],text="VIEWPORT RESTORED"))
    cases.append(run_case("quit-control-c", binary, [(0.15,key(99,modifier=5))]))
    for signum in [signal.SIGINT,signal.SIGTERM,signal.SIGHUP,signal.SIGQUIT,signal.SIGTSTP]:
        cases.append(run_case(f"signal-{signum.name}",binary,[(0.15,lambda p,f,s=signum:os.kill(p,s))]))
    for reply in ["no-graphics","no-releases","silent"]:
        cases.append(run_case(f"reject-{reply}",binary,reply=reply,expect=1,text="Required terminal capabilities",runtime=3.0))
    for fixture,message in [("won","YOU WON"),("game-over","GAME OVER"),("damaged","READY")]:
        cases.append(run_case(f"fixture-{fixture}",["target/release/examples/acceptance",fixture],[(0.15,key(114)),(0.25,key(113))],text=message))
    for fault in ["error","panic"]:
        cases.append(run_case(f"fault-{fault}",["target/release/examples/acceptance",fault],expect=1,text="development injected"))
    for fallback in ["no-compression", "missing-compression"]:
        cases.append(run_case(f"fallback-{fallback}", binary, [(0.5,key(113))],reply=fallback))
    for mode in ["reduced", "off"]:
        cases.append(run_case(f"fx-{mode}", [*binary,"--fx",mode], [(0.1,key(32)),(0.2,key(32)),(0.5,key(113))]))
    cases.append(run_case("effects-scene", ["target/release/examples/acceptance","effects"], [(10.8,key(113))], runtime=13.0,text="YOU WON"))
    cases.append(run_case("fps-60",binary,[(0.05,key(32)),(0.12,key(32)),(1.4,key(113))],runtime=3.0))
    cases.append(run_case("fps-30",[*binary,"--fps","30"],[(0.05,key(32)),(0.12,key(32)),(1.4,key(113))],runtime=3.0))
    result = {"kind":"synthetic-pty-not-ghostty", "cases":cases}
    encoded = json.dumps(result,indent=2) + "\n"
    if args.output:
        args.output.parent.mkdir(parents=True,exist_ok=True)
        args.output.write_text(encoded)
    print(encoded,end="")


if __name__ == "__main__":
    main()
