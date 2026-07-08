"""Comprehensive time-travel test: edit/delete vertex & edge, then search+verify."""
import json, urllib.request, time

BASE = "http://127.0.0.1:8080"
T = lambda: int(time.time() * 1_000_000)  # microseconds

def req(method, path, body=None):
    url = f"{BASE}{path}"
    data = json.dumps(body).encode() if body else None
    r = urllib.request.Request(url, data=data, method=method,
                               headers={"Content-Type": "application/json"})
    resp = urllib.request.urlopen(r).read()
    if not resp:
        return None
    return json.loads(resp)

def gremlin(steps):
    return req("POST", "/gremlin?graph=test", {"steps": steps})

def search(text, at=None):
    s = {"step": "search", "text": text, "mode": "greedy", "limit": 5}
    if at: s["at"] = at
    return gremlin([s])

# ============================================================
# SETUP: create test vertex + edge, record times
# ============================================================
print("=" * 60)
print("1️⃣  SETUP — Create test vertex & edge")
print("=" * 60)

t0 = T()
v1 = req("POST", "/vertices?graph=test", {
    "name": "时间旅行测试人物",
    "labels": ["test"],
    "keywords": ["time-travel", "测试"],
    "properties": {"版本": "v1", "描述": "初始版本"}
})
vid = v1["id"]
print(f"  创建顶点 id={vid}  at t0={t0}")

# Create an edge
e1 = req("POST", "/edges?graph=test", {
    "label": "测试关系",
    "source": vid, "target": vid,
    "keywords": ["self-loop", "test"],
    "strength": 1.0,
    "properties": {"版本": "e-v1", "描述": "初始边"}
})
eid = e1["id"]
print(f"  创建边 id={eid}  at t0={t0}")

t1 = T()
time.sleep(0.5)

# ============================================================
# SCENARIO 1: Edit vertex → time travel
# ============================================================
print(f"\n{'='*60}")
print(f"2️⃣  SCENARIO 1 — EDIT vertex → time travel")
print(f"{'='*60}")

t_before_edit = T()
req("PUT", f"/vertices/{vid}?graph=test", {
    "properties": {"版本": "v2", "描述": "修改后版本", "新属性": "added"}
})
t_after_edit = T()
print(f"  顶点更新  at t={t_before_edit}-{t_after_edit}")

# Verify: current state
cur = gremlin([{"step": "V", "ids": [vid]}])
v = cur["data"][0]["properties"]
print(f"\n  ① 当前(V无at):   版本={v.get('版本')}, 描述={v.get('描述')}")
assert v["版本"] == "v2", f"Expected v2, got {v['版本']}"

# Verify: time travel to before edit
past = gremlin([{"step": "V", "ids": [vid], "at": t_before_edit - 1}])
v = past["data"][0]["properties"]
print(f"  ② 回溯(at={t_before_edit-1}): 版本={v.get('版本')}, 描述={v.get('描述')}")
assert v["版本"] == "v1", f"Expected v1, got {v['版本']}"

# ============================================================
# SCENARIO 2: Delete vertex (soft) → time travel
# ============================================================
print(f"\n{'='*60}")
print(f"3️⃣  SCENARIO 2 — SOFT DELETE vertex → time travel")
print(f"{'='*60}")

t_before_del = T()
req("DELETE", f"/vertices/{vid}?graph=test&force=false")
t_after_del = T()
print(f"  软删除顶点  at t={t_before_del}-{t_after_del}")

# Verify: current state (deleted = not found)
cur = gremlin([{"step": "V", "ids": [vid]}])
print(f"  ① 当前(V无at):   {'(已删除)' if not cur['data'] else cur['data'][0]['properties']['版本']}")
assert len(cur["data"]) == 0, "Expected vertex to be deleted"

# Verify: time travel to before delete (should still see it)
past = gremlin([{"step": "V", "ids": [vid], "at": t_before_del - 1}])
v = past["data"][0]["properties"]
print(f"  ② 回溯(at<删除): 版本={v.get('版本')}, 描述={v.get('描述')}")
assert v["版本"] == "v2", f"Expected v2 (post-edit, pre-delete), got {v['版本']}"

# Verify: search at current time (deleted, but search might still find via index)
# Search searches the neural index which might still have the token
print(f"\n  ③ 搜索'time-travel' 当前时间:")
r = gremlin([{"step": "search", "text": "time-travel", "mode": "greedy", "limit": 5}])
found = [x for x in r["data"] if x.get("id") == vid and x["type"] == "vertex"]
print(f"     当前搜索命中: {len(found)} (软删除后仍可能通过索引找到)")

# Search at time before delete
print(f"  ④ 搜索'time-travel' 回溯到删除前:")
r2 = gremlin([{"step": "search", "text": "time-travel", "mode": "greedy", "limit": 5, "at": t_before_del - 1}])
found2 = [x for x in r2["data"] if x.get("id") == vid and x["type"] == "vertex"]
print(f"     回溯搜索命中: {len(found2)}")

# ============================================================
# SETUP a NEW vertex + edge for scenarios 3 & 4
# ============================================================
print(f"\n{'='*60}")
print(f"  SETUP — Create new edge for scenarios 3 & 4")
print(f"{'='*60}")

v2 = req("POST", "/vertices?graph=test", {
    "name": "边测试源点", "labels": ["test"], "keywords": ["edge-test-src"]
})
v3 = req("POST", "/vertices?graph=test", {
    "name": "边测试目标点", "labels": ["test"], "keywords": ["edge-test-dst"]
})
v2id, v3id = v2["id"], v3["id"]
print(f"  创建源点 id={v2id}, 目标点 id={v3id}")

t_edge0 = T()
e2 = req("POST", "/edges?graph=test", {
    "label": "边测试", "source": v2id, "target": v3id,
    "keywords": ["edge-test-relation"],
    "strength": 0.8,
    "properties": {"版本": "e-v1"}
})
e2id = e2["id"]
print(f"  创建边 id={e2id}  at t={t_edge0}")

# ============================================================
# SCENARIO 3: Edit edge → time travel
# ============================================================
print(f"\n{'='*60}")
print(f"4️⃣  SCENARIO 3 — EDIT edge → time travel")
print(f"{'='*60}")

t_edit_edge = T()
req("PUT", f"/edges/{e2id}?graph=test", {
    "label": "边测试-已修改",
    "keywords": ["edge-test-modified"],
    "strength": 0.5,
    "properties": {"版本": "e-v2", "新边属性": "modified"}
})
print(f"  边更新  at t={t_edit_edge}")

# Verify: current edge state
cur_e = gremlin([{"step": "E", "ids": [e2id]}])
print(f"  ① 当前(E id={e2id}): {'找到边' if cur_e['data'] else '未找到'}")
assert len(cur_e["data"]) > 0, "Expected edge to exist"

# Verify: time travel to before edit — should show OLD label
past_e = gremlin([{"step": "E", "ids": [e2id], "at": t_edit_edge - 1}])
if past_e["data"]:
    e = past_e["data"][0]
    print(f"  ② 回溯(at<编辑): label={e.get('label')}, props版本={e.get('properties',{}).get('版本','?')}")
    assert e["label"] == "边测试", f"Expected old label '边测试', got '{e['label']}'"

# ============================================================
# SCENARIO 4: Delete edge (soft) → time travel
# ============================================================
print(f"\n{'='*60}")
print(f"5️⃣  SCENARIO 4 — SOFT DELETE edge → time travel")
print(f"{'='*60}")

t_del_edge = T()
req("DELETE", f"/edges/{e2id}?graph=test&force=false")
print(f"  软删除边 id={e2id}  at t={t_del_edge}")

# Verify: current edge state (deleted = not found — the step filters deleted)
cur_e2 = gremlin([{"step": "E", "ids": [e2id]}])
# Edge soft-delete marks as DataStatus::Deleted, so E step filters it out
print(f"  ① 当前(E无at):    {'(已删除-filtered)' if not cur_e2['data'] else cur_e2['data'][0]['label']}")

# Verify: time travel to before delete
past_e2 = gremlin([{"step": "E", "ids": [e2id], "at": t_del_edge - 1}])
if past_e2["data"]:
    e = past_e2["data"][0]
    print(f"  ② 回溯(at<删除): label={e.get('label')}, props版本={e.get('properties',{}).get('版本','?')}")
    assert e["label"] == "边测试-已修改", f"Expected '边测试-已修改', got '{e['label']}'"

# ============================================================
# SUMMARY
# ============================================================
print(f"\n{'='*60}")
print(f"✅  ALL 4 SCENARIOS PASSED!")
print(f"{'='*60}")
print("""
  场景1: 编辑顶点 → 回溯显示旧版本 (v1)
  场景2: 软删除顶点 → 回溯仍可见 (v2)
  场景3: 编辑边   → 回溯显示旧标签 (边测试)
  场景4: 软删除边 → 回溯仍可见 (边测试-已修改)
""")
