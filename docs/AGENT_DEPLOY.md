# 服务器 Agent 部署指南

`agent/agent.py` 是一个零依赖的 Python3 单文件采集脚本，暴露 CPU / 内存 / NVIDIA GPU 指标。桌面端 Monitor Plus 通过 HTTP 轮询这些指标。支持两种部署：**Windows 守护模式**（pythonw 无窗口 + 计划任务守护）和 **Linux systemd 服务**。

## Windows 部署（pythonw 无窗口 + 5 分钟守护）

1. 把 `agent/` 目录整个放到固定位置（如 `D:\monitor-agent\`）
2. **双击 `windows-install.cmd`**（或右键 windows-install.ps1 → 使用 PowerShell 运行）

脚本自动完成：

- 定位 `pythonw.exe`（无窗口解释器，不弹黑框；自动跳过微软商店占位目录）
- 注册计划任务 `MonitorPlusAgent`：**每 5 分钟触发一次**——agent 已在运行则跳过，不在运行（崩溃/重启后）则自动用 pythonw 拉起；另有登录触发，开机后自动启动
- 立即启动一次并验证进程

验证：

```powershell
curl http://127.0.0.1:9101/health      # {"ok": true}
schtasks /Query /TN MonitorPlusAgent   # 守护任务状态
```

卸载：双击 `windows-uninstall.cmd`（删任务 + 停进程）。

说明：agent 自带单实例保护——9101 端口已被占用时新拉起的进程静默退出，因此 5 分钟触发不会产生重复实例。之前手动 `python agent.py` 跑的旧实例（带黑框）在装守护前先关掉一次。

## 要求（Linux 路径）

- Linux（读 `/proc/stat`、`/proc/meminfo`）
- Python 3.7+（仅标准库）
- NVIDIA GPU 监控需要 `nvidia-smi`（随驱动安装）；没有 GPU 也能跑，GPU 字段返回空

## Linux 快速部署

先在下载了项目的电脑上上传文件，将 `user@server` 换成自己的 SSH 登录目标：

```bash
scp agent/agent.py agent/monitor-agent.service user@server:/tmp/
ssh user@server
```

再在服务器上安装并启动服务：

```bash
# 在服务器上
sudo mkdir -p /opt/monitor-agent
sudo mv /tmp/agent.py /opt/monitor-agent/
sudo mv /tmp/monitor-agent.service /etc/systemd/system/
sudo systemctl daemon-reload
sudo systemctl enable --now monitor-agent
```

验证：

```bash
curl http://127.0.0.1:9101/metrics
# {"hostname": "gpu-01", "cpu_percent": 12.3, "cpu_cores": [...],
#  "mem_used_gb": 10.2, "mem_total_gb": 64, "gpus": [...], "agent_ts_ms": ...}
```

然后在 Monitor Plus 设置中添加服务器：`http://<服务器IP>:9101`。

## 自定义

直接改 `/etc/systemd/system/monitor-agent.service` 的 `ExecStart`，然后 `systemctl daemon-reload && systemctl restart monitor-agent`：

| 参数 | 默认 | 说明 |
|---|---|---|
| `--bind` | `0.0.0.0` | 监听地址 |
| `--port` | `9101` | 监听端口 |
| `--interval` | `2.0` | 采样间隔（秒）；客户端轮询快于采样间隔只会读到缓存值 |
| `--token` | 空 | 访问令牌；设置后客户端请求必须带 `X-Agent-Token` 头（Monitor Plus 服务器配置里填同一令牌） |

## 安全建议

Agent 默认公开主机负载、主机名和硬件信息，访问令牌默认未启用。请按网络范围配置访问：

1. **纯内网**：保持默认即可，或开 `--token`
2. **跨公网**：通过 VPN、SSH 隧道或 HTTPS 反向代理连接，并配置访问限制：
   - **防火墙白名单**：只放行运行 Monitor Plus 的机器：
     ```bash
     iptables -A INPUT -p tcp --dport 9101 -s <你的IP> -j ACCEPT
     iptables -A INPUT -p tcp --dport 9101 -j DROP
     ```
     云服务器用安全组配置更简单
   - **令牌**：`--token <随机串>`，桌面端配置里填同样令牌。HTTP 上的令牌不提供传输加密。
3. 不要把 9101 端口暴露到公网且不加任何限制

## 故障排查

```bash
systemctl status monitor-agent          # 服务状态
journalctl -u monitor-agent -n 50       # 最近日志
MP_AGENT_VERBOSE=1 /usr/bin/python3 /opt/monitor-agent/agent.py --port 9101   # 前台调试（含访问日志）
nvidia-smi                              # GPU 是否可查
```

桌面端显示"离线"时依次检查：服务是否 running → `curl http://IP:9101/metrics` 是否通 → 防火墙/安全组 → 令牌是否一致。
