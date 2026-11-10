#!/usr/bin/env python3
"""Monitor Plus 采集 Agent（零依赖，仅需 Python 3.7+）

采集本机 CPU / 内存 / NVIDIA GPU 指标，通过内置 HTTP 服务暴露：

    GET /metrics            -> JSON 指标（可选用 X-Agent-Token 头鉴权）
    GET /health             -> {"ok": true}

用法：
    python3 agent.py --port 9101 --bind 0.0.0.0 --token <可选访问令牌>

字段结构与 Monitor Plus 桌面端约定对齐（camelCase）。
"""

import argparse
import json
import os
import re
import socket
import socketserver
import subprocess
import sys
import threading
import time
from http.server import BaseHTTPRequestHandler, HTTPServer

__version__ = "0.1.2"

# pythonw.exe（无窗口运行）下 stdout/stderr 为 None，print 会直接崩；
# 统一重定向到 devnull，保证守护方式拉起时也能正常常驻
if sys.stdout is None:
    sys.stdout = open(os.devnull, "w", encoding="utf-8")
if sys.stderr is None:
    sys.stderr = open(os.devnull, "w", encoding="utf-8")

# ---------------- 采样缓存 ----------------
_lock = threading.Lock()
_cache = {
    "hostname": "",
    "cpu_percent": 0.0,
    "cpu_cores": [],
    "mem_used_gb": None,
    "mem_total_gb": None,
    "gpus": [],
    "agent_ts_ms": 0,
}

# ---------------- /proc 采集 ----------------
_prev_cpu = None  # (total, idle)


def _read_cpu_times():
    """读取 /proc/stat 的 cpu 汇总行与各核心行，返回 [(total, idle), ...]"""
    stats = []
    with open("/proc/stat", "rb") as f:
        for line in f:
            parts = line.decode().split()
            if not re.fullmatch(r"cpu\d*", parts[0]):
                break
            vals = [int(x) for x in parts[1:]]
            # user nice system idle iowait irq softirq steal ...
            idle = vals[3] + (vals[4] if len(vals) > 4 else 0)
            stats.append((sum(vals), idle))
    return stats


def sample_cpu():
    global _prev_cpu
    cur = _read_cpu_times()
    if _prev_cpu is None or len(_prev_cpu) != len(cur):
        _prev_cpu = cur
        return None
    percents = []
    for (t1, i1), (t0, i0) in zip(cur, _prev_cpu):
        dt, di = t1 - t0, i1 - i0
        percents.append(round(max(0.0, min(100.0, (1.0 - di / dt) * 100.0)), 1) if dt > 0 else 0.0)
    _prev_cpu = cur
    return percents


def sample_mem():
    """读 /proc/meminfo，返回 (used_gb, total_gb)；MemAvailable 计算真实可用"""
    info = {}
    with open("/proc/meminfo") as f:
        for line in f:
            key, rest = line.split(":", 1)
            info[key] = int(rest.strip().split()[0])  # kB
    total = info.get("MemTotal")
    avail = info.get("MemAvailable", info.get("MemFree"))
    if not total or avail is None:
        return None, None
    used = total - avail
    return round(used / 1048576, 2), round(total / 1048576, 2)  # kB -> GB


# ---- Windows 采集（ctypes，仅标准库；读每核时间与物理内存）----
_win_cpu_prev = [None]


def _win_cpu_times():
    """通过 ntdll!NtQuerySystemInformation 读取每核 idle/kernel/user 时间（100ns 单位）
    返回 [(total, idle), ...]，total = kernel + user（kernel 已含 idle，与 Linux 口径一致）"""
    import ctypes

    class _SPI(ctypes.Structure):
        _fields_ = [
            ("IdleTime", ctypes.c_longlong),
            ("KernelTime", ctypes.c_longlong),
            ("UserTime", ctypes.c_longlong),
            ("DpcTime", ctypes.c_longlong),
            ("InterruptTime", ctypes.c_longlong),
            ("InterruptCount", ctypes.c_ulong),
        ]

    ncpu = os.cpu_count() or 0
    if ncpu <= 0:
        return None
    buf = (_SPI * ncpu)()
    retlen = ctypes.c_ulong(0)
    status = ctypes.windll.ntdll.NtQuerySystemInformation(
        8, buf, ctypes.sizeof(buf), ctypes.byref(retlen)  # 8 = SystemProcessorPerformanceInformation
    )
    if status != 0:
        return None
    return [(s.KernelTime + s.UserTime, s.IdleTime) for s in buf]


def sample_cpu_windows():
    global _win_cpu_prev
    cur = _win_cpu_times()
    if cur is None:
        return None
    if _win_cpu_prev[0] is None or len(_win_cpu_prev[0]) != len(cur):
        _win_cpu_prev[0] = cur
        return None
    percents = []
    for (t1, i1), (t0, i0) in zip(cur, _win_cpu_prev[0]):
        dt, di = t1 - t0, i1 - i0
        percents.append(round(max(0.0, min(100.0, (1.0 - di / dt) * 100.0)), 1) if dt > 0 else 0.0)
    _win_cpu_prev[0] = cur
    return percents


def sample_mem_windows():
    """GlobalMemoryStatusEx，返回 (used_gb, total_gb)"""
    import ctypes

    class _MS(ctypes.Structure):
        _fields_ = [
            ("dwLength", ctypes.c_ulong),
            ("dwMemoryLoad", ctypes.c_ulong),
            ("ullTotalPhys", ctypes.c_ulonglong),
            ("ullAvailPhys", ctypes.c_ulonglong),
            ("ullTotalPageFile", ctypes.c_ulonglong),
            ("ullAvailPageFile", ctypes.c_ulonglong),
            ("ullTotalVirtual", ctypes.c_ulonglong),
            ("ullAvailVirtual", ctypes.c_ulonglong),
            ("ullAvailExtendedVirtual", ctypes.c_ulonglong),
        ]

    ms = _MS()
    ms.dwLength = ctypes.sizeof(_MS)
    if not ctypes.windll.kernel32.GlobalMemoryStatusEx(ctypes.byref(ms)):
        return None, None
    return round((ms.ullTotalPhys - ms.ullAvailPhys) / 1024 ** 3, 2), round(ms.ullTotalPhys / 1024 ** 3, 2)


# ---------------- nvidia-smi 采集 ----------------
NVIDIA_FIELDS = ["index", "name", "utilization.gpu", "memory.used", "memory.total", "temperature.gpu"]


def _subprocess_run(cmd, *, timeout):
    """统一执行外部采集命令；pythonw 下绝不创建控制台窗口。"""
    kwargs = {"capture_output": True, "text": True, "timeout": timeout}
    if os.name == "nt":
        kwargs["creationflags"] = subprocess.CREATE_NO_WINDOW
    return subprocess.run(cmd, **kwargs)


def sample_gpus():
    """调 nvidia-smi 查询 GPU；不可用时返回 []"""
    try:
        out = _subprocess_run(
            ["nvidia-smi", f"--query-gpu={','.join(NVIDIA_FIELDS)}", "--format=csv,noheader,nounits"],
            timeout=5,
        )
        if out.returncode != 0:
            return []
        gpus = []
        for line in out.stdout.strip().splitlines():
            parts = [p.strip() for p in line.split(",")]
            if len(parts) < 5:
                continue
            try:
                gpus.append({
                    "index": int(float(parts[0])),
                    "name": parts[1],
                    "util": float(parts[2]),
                    "mem_used_mb": float(parts[3]),
                    "mem_total_mb": float(parts[4]),
                    "temp_c": float(parts[5]) if len(parts) > 5 and parts[5] != "[N/A]" else None,
                })
            except ValueError:
                continue
        return gpus
    except (OSError, subprocess.SubprocessError):
        return []


def get_hostname():
    if os.name != "nt":
        try:
            with open("/proc/sys/kernel/hostname") as f:
                name = f.read().strip()
                if name:
                    return name
        except OSError:
            pass
    return socket.gethostname() or "unknown"


def _run(cmd, timeout=5):
    try:
        out = _subprocess_run(cmd, timeout=timeout)
        return out.stdout if out.returncode == 0 else ""
    except (OSError, subprocess.SubprocessError):
        return ""


# ---- macOS 降级采集（仅用于本机联调；服务器主路径走 /proc 与 nvidia-smi）----
_mac_cpu_prev = [None]


def sample_cpu_macos():
    out = _run(["top", "-l", "1", "-n", "0"])
    m = re.search(r"CPU usage:\s*([\d.]+)% user.*?([\d.]+)% sys.*?([\d.]+)% idle", out)
    if not m:
        return []
    user, sys_, idle = (float(m.group(i)) for i in (1, 2, 3))
    return [round(max(0.0, min(100.0, user + sys_)), 1)]


def sample_mem_macos():
    total = 0
    out = _run(["sysctl", "-n", "hw.memsize"])
    if out.strip().isdigit():
        total = int(out.strip())
    if not total:
        return None, None
    out = _run(["vm_stat"])
    if not out:
        return None, None
    pm = re.search(r"page size of (\d+) bytes", out)
    page = int(pm.group(1)) if pm else 4096

    def pages(label):
        m = re.search(rf"{label}:\s*(\d+)", out)
        return int(m.group(1)) if m else 0

    free = pages("Pages free") + pages("Pages inactive") + pages("Pages speculative")
    used = max(0, total - free * page)
    return round(used / 1024**3, 2), round(total / 1024**3, 2)


def sampler_loop(interval, stop_event=None):
    """持续采样。

    stop_event 仅供测试和嵌入方优雅停止；常规 Agent 进程保持默认 None，
    由进程退出结束守护线程。
    """
    import os
    is_linux = os.path.isfile("/proc/stat")
    is_windows = os.name == "nt"
    while stop_event is None or not stop_event.is_set():
        started = time.time()
        try:
            if is_linux:
                cores = sample_cpu() or []
                mem_used, mem_total = sample_mem()
            elif is_windows:
                cores = sample_cpu_windows() or []
                mem_used, mem_total = sample_mem_windows()
            else:
                cores = sample_cpu_macos()
                mem_used, mem_total = sample_mem_macos()
            # nvidia-smi 可能在慢 GPU 上阻塞数秒，必须在共享锁外查询，
            # 让 /metrics 随时立即返回上一次完整快照。
            gpus = sample_gpus()
            sampled_at_ms = int(time.time() * 1000)
            with _lock:
                _cache["cpu_cores"] = cores
                _cache["cpu_percent"] = round(sum(cores) / len(cores), 1) if cores else 0.0
                _cache["mem_used_gb"], _cache["mem_total_gb"] = mem_used, mem_total
                _cache["gpus"] = gpus
                _cache["agent_ts_ms"] = sampled_at_ms
        except Exception as e:  # 采样失败不中断线程
            print(f"sampler error: {e}", flush=True)
        elapsed = time.time() - started
        delay = max(0.5, interval - elapsed)
        # Event.wait 让测试可以在 patch 仍生效时及时停止采样线程。
        if stop_event is None:
            time.sleep(delay)
        else:
            stop_event.wait(delay)


# ---------------- HTTP 服务 ----------------
class Handler(BaseHTTPRequestHandler):
    server_version = f"MonitorPlusAgent/{__version__}"

    def _send(self, code, payload):
        body = json.dumps(payload).encode()
        self.send_response(code)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)

    def do_GET(self):
        expected = getattr(self.server, "token", None)
        if expected:
            provided = self.headers.get("X-Agent-Token", "")
            if provided != expected:
                self._send(401, {"error": "invalid token"})
                return
        if self.path == "/metrics":
            with _lock:
                payload = dict(_cache)
            self._send(200, payload)
        elif self.path == "/health":
            self._send(200, {"ok": True})
        else:
            self._send(404, {"error": "not found"})

    def log_message(self, fmt, *args):
        if os.environ.get("MP_AGENT_VERBOSE"):
            print(f"{self.address_string()} {fmt % args}", flush=True)


class Server(socketserver.ThreadingMixIn, HTTPServer):
    daemon_threads = True

    # Windows 的 SO_REUSEADDR 语义会允许多个 socket 绑定同一端口。
    # 监听服务必须使用互斥绑定；Unix 上保留它主要用于 TIME_WAIT 后快速重启。
    allow_reuse_address = os.name != "nt"

    def server_bind(self):
        if os.name == "nt":
            # 必须在 bind 前设置。该选项阻止另一个进程抢占/重复绑定本端口。
            self.socket.setsockopt(socket.SOL_SOCKET, socket.SO_EXCLUSIVEADDRUSE, 1)
        super().server_bind()


def main():
    ap = argparse.ArgumentParser(description="Monitor Plus 采集 Agent")
    ap.add_argument("--bind", default="0.0.0.0", help="监听地址（默认 0.0.0.0）")
    ap.add_argument("--port", type=int, default=9101, help="监听端口（默认 9101）")
    ap.add_argument("--interval", type=float, default=2.0, help="采样间隔秒数（默认 2）")
    ap.add_argument("--token", default="", help="可选访问令牌（客户端需带 X-Agent-Token 头）")
    args = ap.parse_args()

    try:
        srv = Server((args.bind, args.port), Handler)
    except OSError:
        # 端口被占用 = 已有实例在跑（每 5 分钟守护触发器拉起时走这里），静默退出
        print(f"port {args.bind}:{args.port} already in use, another agent is running, exit",
              flush=True)
        return
    srv.token = args.token if args.token else None

    with _lock:
        _cache["hostname"] = get_hostname()
    threading.Thread(target=sampler_loop, args=(args.interval,), daemon=True).start()

    print(f"Monitor Plus agent listening on {args.bind}:{args.port} "
          f"(interval={args.interval}s, token={'on' if args.token else 'off'})", flush=True)
    try:
        srv.serve_forever()
    except KeyboardInterrupt:
        pass


if __name__ == "__main__":
    main()
