"""
崩溃一致性测试：修改顶点/边 name 后 kill -9，重启验证 name 是否持久化
"""
import time, os, signal, subprocess, sys
from bionic_graph import Client

BASE_URL = "http://127.0.0.1:8080"
GRAPH = "crash_test"
BINARY = "/tmp/bionic-graph/target/debug/bionic-graph"
SERVER_LOG = "/tmp/bg-server.log"

client = Client(base_url=BASE_URL, timeout=30)

def get_vertex_name(vid):
    r = client.execute_gremlin([{"step": "V", "ids": [vid]}], graph=GRAPH)
    if r.data:
        return r.data[0].name
    return ""

def get_edge_name(eid):
    r = client.execute_gremlin([{"step": "E", "ids": [eid]}], graph=GRAPH)
    if r.data:
        return r.data[0].name
    return ""

# 创建测试图
client.create_graph(GRAPH, description="crash consistency test")
print(f"[setup] 图 '{GRAPH}' 已创建")

# 创建顶点和边
v1 = client.create_vertex("Alice", labels=["person"], graph=GRAPH)
v2 = client.create_vertex("Bob", labels=["person"], graph=GRAPH)
e1 = client.create_edge(v1.id, v2.id, "friend", graph=GRAPH)
print(f"[setup] 顶点: Alice(id={v1.id}), Bob(id={v2.id})")
print(f"[setup] 边: friend(id={e1.id})")

# 验证初始 name
assert get_vertex_name(v1.id) == "Alice", f"初始顶点 name 不符"
assert get_edge_name(e1.id) == "friend", f"初始边 name 不符"
print("[setup] ✅ 初始 name 正确")

# 修改 name
print("\n[test] 修改顶点 name: Alice → Alice Wang")
client.update_vertex_meta(v1.id, {"name": "Alice Wang"}, graph=GRAPH)
print("[test] 修改边 name: friend → close_friend")
client.update_edge_meta(e1.id, {"name": "close_friend"}, graph=GRAPH)

# 验证修改后值
assert get_vertex_name(v1.id) == "Alice Wang", "修改后顶点 name 不符"
assert get_edge_name(e1.id) == "close_friend", "修改后边 name 不符"
print("[verify] ✅ 修改后 name 正确: Alice Wang / close_friend")

# WAL 同步：等待写入完成
time.sleep(2)

# kill -9
pid = subprocess.check_output(["pgrep", "-x", "bionic-graph"]).decode().strip()
print(f"\n[crash] kill -9 PID {pid} ...")
os.kill(int(pid), signal.SIGKILL)
time.sleep(2)
try:
    os.kill(int(pid), 0)
    print("[crash] ❌ 进程仍然存活！")
    sys.exit(1)
except OSError:
    print("[crash] ✅ 进程已杀死")

# 重启服务
print("\n[restart] 启动服务 ...")
proc = subprocess.Popen([BINARY], stdout=open(SERVER_LOG, "a"), stderr=subprocess.STDOUT)
time.sleep(4)

# 验证 name 持久化
client2 = Client(base_url=BASE_URL, timeout=30)
v_name = get_vertex_name(v1.id)
e_name = get_edge_name(e1.id)
print(f"[result] 重启后顶点 name: '{v_name}'")
print(f"[result] 重启后边 name: '{e_name}'")

if v_name == "Alice Wang" and e_name == "close_friend":
    print("\n[result] ✅ 崩溃一致性测试通过！name 已正确持久化")
else:
    print("\n[result] ❌ 测试失败！")
    if v_name != "Alice Wang":
        print(f"  顶点 name 期望: 'Alice Wang', 实际: '{v_name}'")
    if e_name != "close_friend":
        print(f"  边 name 期望: 'close_friend', 实际: '{e_name}'")

# 清理
client2.delete_graph(GRAPH)
print("[cleanup] 测试图已删除")
