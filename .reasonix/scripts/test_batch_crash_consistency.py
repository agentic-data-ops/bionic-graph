#!/usr/bin/env python3
"""
WAL 批量模式崩溃一致性测试（kill -9）

测试场景：在 batch_load（批量导入）执行期间用 SIGKILL 杀死后端进程，
模拟生产环境中的突然崩溃。重启后验证：
  1. 数据文件自一致性（rebuild 不崩溃）
  2. Gremlin 查询可用
  3. 新写入正常

与 test_crash_consistency.py 的区别：该测试针对单条 WAL 写入后 kill，
本测试针对 batch_load 路径（多条 WAL 记录在内存中累积后统一 flush）。
"""

import os
import sys
import time
import json
import signal
import socket
import subprocess
import threading
from urllib.parse import urljoin

import httpx

# ── 配置 ─────────────────────────────────────────────────────────────

BASE_URL = os.environ.get("BG_BASE_URL", "http://127.0.0.1:8080")
GRAPH = "crash_batch_test"
BINARY = os.environ.get(
    "BG_BINARY",
    "/tmp/bionic-graph/target/debug/bionic-graph",
)
SERVER_LOG = "/tmp/bg-server-crash-batch.log"
DATA_DIR = "/tmp/bionic-graph/data"

# 批量导入量：数量越大，batch 处理时间窗口越长
BATCH_SIZE = 50000


# ── 工具函数 ─────────────────────────────────────────────────────────

def server_pid():
    """返回 bionic-graph 进程 PID，未找到返回 None。"""
    try:
        out = subprocess.check_output(["pgrep", "-x", "bionic-graph"], timeout=5)
        return int(out.decode().strip())
    except (subprocess.CalledProcessError, FileNotFoundError, ValueError):
        return None


def wait_for_server(url: str, timeout: float = 15.0) -> bool:
    """等待服务就绪，返回是否成功。"""
    deadline = time.time() + timeout
    while time.time() < deadline:
        try:
            r = httpx.get(urljoin(url, "/health"), timeout=3)
            if r.status_code == 200:
                return True
        except (httpx.ConnectError, httpx.TimeoutException):
            pass
        time.sleep(0.3)
    return False


def start_server() -> subprocess.Popen | None:
    """启动后端服务（如果未运行）。"""
    pid = server_pid()
    if pid is not None:
        print(f"[server] 服务已在运行 PID={pid}")
        return None

    print(f"[server] 启动: {BINARY}")
    proc = subprocess.Popen(
        [BINARY],
        stdout=open(SERVER_LOG, "a"),
        stderr=subprocess.STDOUT,
    )
    if wait_for_server(BASE_URL):
        print(f"[server] 服务已就绪 PID={proc.pid}")
        return proc
    print("[server] ❌ 服务启动超时")
    return None


def kill_server(sig: int = signal.SIGKILL):
    """杀死 bionic-graph 进程。"""
    pid = server_pid()
    if pid is None:
        print("[kill] 未找到运行中的服务进程")
        return
    print(f"[kill] 发送信号 {sig} 到 PID {pid} ...")
    os.kill(pid, sig)
    time.sleep(1)
    # 确认已死
    try:
        os.kill(pid, 0)
        print(f"[kill] ⚠️ 进程 {pid} 仍然存活，重试 SIGKILL")
        os.kill(pid, signal.SIGKILL)
        time.sleep(1)
    except OSError:
        pass
    print(f"[kill] ✅ 进程 {pid} 已终止")


def gremlin_query(client: httpx.Client, steps: list, graph: str = GRAPH) -> dict:
    """执行 Gremlin 查询并返回结果。"""
    resp = client.post(
        urljoin(BASE_URL, "/gremlin"),
        json={"steps": steps},
        headers={"X-Graph-Name": graph},
        timeout=10,
    )
    resp.raise_for_status()
    return resp.json()


# ── 测试步骤 ─────────────────────────────────────────────────────────

def step_ping(client: httpx.Client):
    """检查服务健康。"""
    resp = client.get(urljoin(BASE_URL, "/health"), timeout=5)
    resp.raise_for_status()
    print(f"[step 0] ✅ 服务健康: {resp.json()}")


def step_create_graph(client: httpx.Client):
    """创建测试图。"""
    resp = client.post(
        urljoin(BASE_URL, "/graphs"),
        json={"name": GRAPH, "description": "WAL batch crash test"},
        timeout=5,
    )
    resp.raise_for_status()
    print(f"[step 1] ✅ 测试图 '{GRAPH}' 已创建")


def step_fire_batch_load(client: httpx.Client, stop_event: threading.Event):
    """在后台发送 batch_load 请求。

    构造大量实体使处理时间足够长，以便主线程能捕获到 kill 时机。
    batch_load 在服务端会启用 WAL batch 模式（start_batch）。
    如果 kill 发生在 end_batch 之前，WAL 条目丢失，需要依靠
    数据文件中的脏块恢复。
    """
    entities = [
        {
            "name": f"CrashNode_{i}",
            "labels": ["test", "crash"],
            "keywords": [f"kw_{i % 100}"],
            "properties": {"index": i, "payload": "x" * 50},
        }
        for i in range(BATCH_SIZE)
    ]

    try:
        print(f"[step 2] 🚀 发起 batch_load ({BATCH_SIZE} 个实体)...")
        resp = client.post(
            urljoin(BASE_URL, "/batch/load"),
            json={"entities": entities, "relations": [], "update_existing": False},
            headers={"X-Graph-Name": GRAPH},
            timeout=300,
        )
        # 如果请求成功（没被杀），记录结果
        if resp.status_code == 200:
            data = resp.json()
            print(f"[step 2] batch_load 完成: {json.dumps(data, ensure_ascii=False)}")
        else:
            print(f"[step 2] batch_load 响应: HTTP {resp.status_code} {resp.text[:100]}")
    except (httpx.ReadTimeout, httpx.ConnectError, httpx.RemoteProtocolError) as e:
        # 服务被杀导致的连接断开 —— 预期行为
        print(f"[step 2] ⚡ 连接中断（预期行为）: {type(e).__name__}")
    except Exception as e:
        print(f"[step 2] ⚡ 其他异常（预期行为）: {type(e).__name__}: {e}")
    finally:
        stop_event.set()


def step_crash_kill():
    """在 batch_load 处理期间直接 kill -9。"""
    # 等待 1 秒确保 batch_load 已开始处理
    time.sleep(1.5)
    kill_server(signal.SIGKILL)
    # 再确认一次
    time.sleep(0.5)
    assert server_pid() is None, "服务进程仍然存活"
    print("[step 3] ✅ 服务已被 SIGKILL 终止")


def step_restart():
    """重启服务。"""
    proc = start_server()
    if proc is None and server_pid() is None:
        print("[step 4] ❌ 服务重启失败")
        sys.exit(1)
    print("[step 4] ✅ 服务已重启")


def step_verify_consistency(client: httpx.Client):
    """验证崩溃后数据自一致性。

    检查要点：
    1. 图存在 → 后端 rebuild 顺利（若不存在则重新创建）
    2. Gremlin V 查询不报错 → 内存索引重建正确
    3. 能够创建新顶点 → 引擎功能完整
    """
    errors = []

    # 检查图是否存在
    graph_exists = False
    try:
        resp = client.get(urljoin(BASE_URL, "/graphs"), timeout=5)
        resp.raise_for_status()
        graphs = resp.json()
        names = [g.get("name") for g in graphs.get("graphs", [])]
        graph_exists = GRAPH in names
        if graph_exists:
            print(f"[step 5.1] ✅ 图 '{GRAPH}' 存在（rebuild 成功）")
        else:
            print(f"[step 5.1] ℹ️  图 '{GRAPH}' 未在 crash 中存活，重新创建")
            resp = client.post(
                urljoin(BASE_URL, "/graphs"),
                json={"name": GRAPH},
                timeout=5,
            )
            resp.raise_for_status()
            graph_exists = True
            print(f"[step 5.1] ✅ 图 '{GRAPH}' 已重新创建")
    except Exception as e:
        errors.append(f"检查/创建图失败: {e}")

    if not graph_exists:
        errors.append("无法获取或创建测试图")

    # Gremlin 查询 —— 验证内存索引正确重建
    try:
        r = gremlin_query(client, [{"step": "V", "limit": 10}])
        if r.get("success", False):
            count = len(r.get("data", []))
            print(f"[step 5.2] ✅ Gremlin V 查询成功，返回 {count} 条")
        else:
            err = r.get("error", "unknown")
            errors.append(f"Gremlin 查询失败: {err}")
    except Exception as e:
        errors.append(f"Gremlin 请求异常: {e}")

    # 创建新顶点 —— 验证引擎功能完整
    try:
        resp = client.post(
            urljoin(BASE_URL, "/vertices"),
            json={"name": "PostCrashNode", "labels": ["test"]},
            headers={"X-Graph-Name": GRAPH},
            timeout=5,
        )
        resp.raise_for_status()
        new_id = resp.json().get("id")
        print(f"[step 5.3] ✅ 新顶点创建成功 id={new_id}")
    except Exception as e:
        errors.append(f"创建顶点失败: {e}")

    # 边查询
    try:
        r = gremlin_query(client, [{"step": "E", "limit": 10}])
        if r.get("success", False):
            count = len(r.get("data", []))
            print(f"[step 5.4] ✅ Gremlin E 查询成功，返回 {count} 条")
        else:
            err = r.get("error", "unknown")
            errors.append(f"Gremlin 边查询失败: {err}")
    except Exception as e:
        errors.append(f"Gremlin 边请求异常: {e}")

    # 显示幸存数据量
    try:
        r = gremlin_query(client, [{"step": "V", "limit": 100}])
        if r.get("success", False):
            total = len(r.get("data", []))
            print(f"[step 5.5] ℹ️  重启后幸存顶点数: {total}")
    except Exception:
        pass

    if errors:
        print(f"\n[result] ❌ 一致性检查失败:")
        for e in errors:
            print(f"  - {e}")
        sys.exit(1)

    print(f"\n[result] ✅ 崩溃一致性测试通过！数据文件自一致，引擎功能完整")


def step_cleanup(client: httpx.Client):
    """清理测试图。"""
    try:
        resp = client.delete(
            urljoin(BASE_URL, f"/graphs/{GRAPH}?force=true"),
            timeout=5,
        )
        resp.raise_for_status()
        print(f"[cleanup] ✅ 测试图 '{GRAPH}' 已删除")
    except Exception as e:
        print(f"[cleanup] ⚠️ 清理失败（可忽略）: {e}")


# ── 主流程 ────────────────────────────────────────────────────────────

def main():
    print("=" * 60)
    print("  WAL 批量模式崩溃一致性测试")
    print("=" * 60)

    # 清理上一次残留
    try:
        pid = server_pid()
        if pid:
            kill_server(signal.SIGKILL)
            time.sleep(1)
    except Exception:
        pass

    # 0. 启动服务
    start_server()

    client = httpx.Client(base_url=BASE_URL, timeout=30)

    try:
        step_ping(client)
        step_create_graph(client)

        # 2. 后台发起 batch_load
        stop_event = threading.Event()
        batch_thread = threading.Thread(
            target=step_fire_batch_load,
            args=(client, stop_event),
            daemon=True,
        )
        batch_thread.start()

        # 3. 在 batch_load 处理期间 kill -9
        step_crash_kill()

        # 等待线程结束
        batch_thread.join(timeout=5)

        # 4. 重启
        step_restart()

        # 5. 验证
        client2 = httpx.Client(base_url=BASE_URL, timeout=30)
        try:
            step_verify_consistency(client2)
        finally:
            client2.close()

    finally:
        step_cleanup(client)
        client.close()

    print("\n✨ 测试完成")


if __name__ == "__main__":
    main()
