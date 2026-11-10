"""Focused tests for the Monitor Plus collection agent.

Run from the repository root:

    python -m unittest discover -s agent
"""

import json
import os
import socket
import subprocess
import threading
import time
import unittest
import urllib.error
import urllib.request
from pathlib import Path
from unittest.mock import call, patch

import agent


class HttpAuthTests(unittest.TestCase):
    def setUp(self):
        self.server = agent.Server(("127.0.0.1", 0), agent.Handler)
        self.server.token = "secret-token"
        self.port = self.server.server_port
        self.thread = threading.Thread(target=self.server.serve_forever, daemon=True)
        self.thread.start()

        with agent._lock:
            agent._cache["hostname"] = "test-host"
            agent._cache["cpu_percent"] = 7.3
            agent._cache["agent_ts_ms"] = 123456

    def tearDown(self):
        self.server.shutdown()
        self.server.server_close()
        self.thread.join(timeout=2)

    def _get(self, path, token=None):
        request = urllib.request.Request(f"http://127.0.0.1:{self.port}{path}")
        if token is not None:
            request.add_header("X-Agent-Token", token)
        try:
            with urllib.request.urlopen(request, timeout=2) as response:
                return response.status, json.loads(response.read().decode("utf-8"))
        except urllib.error.HTTPError as error:
            return error.code, json.loads(error.read().decode("utf-8"))

    def test_health_and_metrics_require_exact_token(self):
        self.assertEqual((401, {"error": "invalid token"}), self._get("/health"))
        self.assertEqual((401, {"error": "invalid token"}), self._get("/metrics"))
        self.assertEqual((401, {"error": "invalid token"}), self._get("/metrics", "wrong"))

        status, health = self._get("/health", "secret-token")
        self.assertEqual(200, status)
        self.assertTrue(health["ok"])

        status, metrics = self._get("/metrics", "secret-token")
        self.assertEqual(200, status)
        self.assertEqual(7.3, metrics["cpu_percent"])

    def test_server_without_token_stays_open(self):
        self.server.token = None
        status, health = self._get("/health")
        self.assertEqual(200, status)
        self.assertTrue(health["ok"])


class SlowGpuSamplingTests(unittest.TestCase):
    def test_metrics_are_served_while_slow_gpu_query_is_running(self):
        self.server = agent.Server(("127.0.0.1", 0), agent.Handler)
        self.server.token = "sampling-token"
        self.port = self.server.server_port
        self.thread = threading.Thread(target=self.server.serve_forever, daemon=True)
        self.thread.start()

        gpu_started = threading.Event()
        gpu_finished = threading.Event()
        updated_gpu = [{"index": 0, "name": "mock-gpu", "util": 55.0}]

        def fake_sample_gpus():
            gpu_started.set()
            time.sleep(0.6)
            gpu_finished.set()
            return updated_gpu

        patches = patch.multiple(
            agent,
            sample_cpu=lambda: [10.0],
            sample_mem=lambda: (1.0, 2.0),
            sample_cpu_windows=lambda: [10.0],
            sample_mem_windows=lambda: (1.0, 2.0),
            sample_cpu_macos=lambda: [10.0],
            sample_mem_macos=lambda: (1.0, 2.0),
            sample_gpus=fake_sample_gpus,
        )

        with agent._lock:
            agent._cache["cpu_percent"] = 99.9
            agent._cache["cpu_cores"] = [99.9]
            agent._cache["mem_used_gb"] = 99.0
            agent._cache["mem_total_gb"] = 100.0
            agent._cache["gpus"] = []
            agent._cache["agent_ts_ms"] = 1

        sampler = None
        stop_event = threading.Event()
        try:
            with patches:
                sampler = threading.Thread(
                    target=agent.sampler_loop,
                    args=(0.5, stop_event),
                    daemon=True,
                )
                sampler.start()
                self.assertTrue(gpu_started.wait(timeout=2))

                request = urllib.request.Request(
                    f"http://127.0.0.1:{self.port}/metrics",
                    headers={"X-Agent-Token": "sampling-token"},
                )
                started = time.monotonic()
                with urllib.request.urlopen(request, timeout=2) as response:
                    payload = json.loads(response.read().decode("utf-8"))
                elapsed = time.monotonic() - started

                # The stale complete snapshot must be returned immediately while
                # the mocked slow nvidia-smi call is still blocked.
                self.assertLess(elapsed, 0.3)
                self.assertEqual(99.9, payload["cpu_percent"])
                self.assertEqual([], payload["gpus"])
                self.assertTrue(gpu_finished.wait(timeout=1))

                deadline = time.monotonic() + 1
                while time.monotonic() < deadline:
                    with agent._lock:
                        if agent._cache["gpus"] == updated_gpu:
                            snapshot = dict(agent._cache)
                            break
                    time.sleep(0.01)
                else:
                    self.fail("completed sample was not published")

            self.assertEqual([10.0], snapshot["cpu_cores"])
            self.assertEqual(10.0, snapshot["cpu_percent"])
            self.assertEqual(1.0, snapshot["mem_used_gb"])
            self.assertEqual(2.0, snapshot["mem_total_gb"])
            self.assertEqual(updated_gpu, snapshot["gpus"])
            self.assertGreater(snapshot["agent_ts_ms"], 1)
            self.assertLess(
                abs(snapshot["agent_ts_ms"] - time.time() * 1000), 2000
            )
        finally:
            # 先在 patch 仍可能生效的上下文外显式停止，避免守护线程泄漏后
            # 在测试结束、mock 失效时访问真实 nvidia-smi。
            if sampler is not None:
                stop_event.set()
                sampler.join(timeout=2)
                self.assertFalse(sampler.is_alive(), "sampler thread did not stop")
            self.server.shutdown()
            self.server.server_close()
            self.thread.join(timeout=2)


class PortBindingTests(unittest.TestCase):
    def test_second_listener_on_same_port_is_rejected(self):
        first = agent.Server(("127.0.0.1", 0), agent.Handler)
        try:
            port = first.server_port
            with self.assertRaises(OSError):
                agent.Server(("127.0.0.1", port), agent.Handler)
        finally:
            first.server_close()

    def test_windows_binding_is_exclusive(self):
        if os.name != "nt":
            self.skipTest("SO_EXCLUSIVEADDRUSE is a Windows-specific socket option")
        self.assertFalse(agent.Server.allow_reuse_address)

        first = agent.Server(("127.0.0.1", 0), agent.Handler)
        try:
            exclusive_value = first.socket.getsockopt(
                socket.SOL_SOCKET, socket.SO_EXCLUSIVEADDRUSE
            )
            self.assertEqual(1, exclusive_value)
        finally:
            first.server_close()


class SubprocessHelperTests(unittest.TestCase):
    def test_gpu_and_legacy_helpers_share_no_window_subprocess_helper(self):
        gpu_result = subprocess.CompletedProcess([], 0)
        gpu_result.stdout = "0,Mock GPU,10,100,200,50\n"
        legacy_result = subprocess.CompletedProcess([], 0)
        legacy_result.stdout = "ok"

        with patch.object(agent.subprocess, "run") as run_mock:
            run_mock.side_effect = [gpu_result, legacy_result]
            gpus = agent.sample_gpus()
            output = agent._run(["mock-command"])

        self.assertEqual(2, run_mock.call_count)
        self.assertEqual(
            [
                (
                    ["nvidia-smi", f"--query-gpu={','.join(agent.NVIDIA_FIELDS)}",
                     "--format=csv,noheader,nounits"],
                ),
                (["mock-command"],),
            ],
            [call.args for call in run_mock.call_args_list],
        )
        self.assertEqual(5, run_mock.call_args_list[0].kwargs["timeout"])
        self.assertEqual(5, run_mock.call_args_list[1].kwargs["timeout"])
        if os.name == "nt":
            self.assertEqual(
                subprocess.CREATE_NO_WINDOW,
                run_mock.call_args_list[0].kwargs["creationflags"],
            )
            self.assertEqual(
                subprocess.CREATE_NO_WINDOW,
                run_mock.call_args_list[1].kwargs["creationflags"],
            )
        else:
            self.assertNotIn("creationflags", run_mock.call_args_list[0].kwargs)
            self.assertNotIn("creationflags", run_mock.call_args_list[1].kwargs)

        self.assertEqual(
            [{"index": 0, "name": "Mock GPU", "util": 10.0,
              "mem_used_mb": 100.0, "mem_total_mb": 200.0, "temp_c": 50.0}],
            gpus,
        )
        self.assertEqual("ok", output)


if __name__ == "__main__":
    unittest.main()
