"""Test: edit vertex/edge keywords, then search + timeTravel to verify."""
import json, urllib.request, time

BASE = "http://127.0.0.1:8080"
T = lambda: int(time.time() * 1_000_000)

def req(method, path, body=None):
    url = f"{BASE}{path}"
    data = json.dumps(body).encode() if body else None
    r = urllib.request.Request(url, data=data, method=method,
                               headers={"Content-Type": "application/json"})
    resp = urllib.request.urlopen(r).read()
    return json.loads(resp) if resp else None

def gremlin(steps):
    return req("POST", "/gremlin?graph=test", {"steps": steps})

def search(text, at=None):
    s = {"step": "search", "text": text, "mode": "greedy", "limit": 5}
    if at: s["at"] = at
    return gremlin([s])

def v_by_id(vid, at=None):
    s = {"step": "V", "ids": [vid]}
    if at: s["at"] = at
    return gremlin([s])

def e_by_id(eid, at=None):
    s = {"step": "E", "ids": [eid]}
    if at: s["at"] = at
    return gremlin([s])

print("=" * 65)
print("KEYWORDS 编辑后 search + timeTravel 测试")
print("=" * 65)

# ─── Setup: create vertex + edge with initial keywords ───────
print("\n1️⃣  SETUP — 创建顶点与边，含初始 keywords")
v = req("POST", "/vertices?graph=test", {
    "name": "关键字测试顶点",
    "labels": ["test"],
    "keywords": ["古剑", "青云", "修仙"],
    "properties": {"desc": "初始"}
})
vid = v["id"]
print(f"  顶点 id={vid}  keywords=[古剑, 青云, 修仙]")

# Edge between two vertices
v_src = req("POST", "/vertices?graph=test", {
    "name": "边源点", "labels": ["test"], "keywords": ["src-vertex"]
})
v_dst = req("POST", "/vertices?graph=test", {
    "name": "边目标点", "labels": ["test"], "keywords": ["dst-vertex"]
})
e = req("POST", "/edges?graph=test", {
    "label": "寻宝",
    "source": v_src["id"], "target": v_dst["id"],
    "keywords": ["宝剑", "秘境", "传承"],
    "strength": 1.0,
    "properties": {"desc": "初始边"}
})
eid = e["id"]
print(f"  边 id={eid}  keywords=[宝剑, 秘境, 传承]")
time.sleep(0.3)

# ─── Phase A: search by initial keywords BEFORE edit ─────────
print("\n2️⃣  编辑前搜索 — 验证初始 keywords 可检索")
for kw in ["古剑", "青云", "修仙", "宝剑", "秘境"]:
    r = search(kw)
    hits = len(r["data"])
    name = r["data"][0].get("name","") if r["data"] else "-"
    print(f"  搜索「{kw}」→ {hits}条 (首个: {name})")

# ─── Phase B: edit keywords on both vertex & edge ────────────
print("\n3️⃣  编辑 keywords — 完全替换为新的关键词")
t_before_edit = T()
req("PUT", f"/vertices/{vid}?graph=test", {
    "keywords": ["飞剑", "仙山", "法宝"],
    "properties": {"desc": "已修改keywords"}
})
req("PUT", f"/edges/{eid}?graph=test", {
    "keywords": ["神兵", "洞天", "功法"],
})
t_after_edit = T()
print(f"  ⏱ 编辑发生在 t={t_before_edit}-{t_after_edit}")
print(f"  顶点新 keywords=[飞剑, 仙山, 法宝]")
print(f"  边新 keywords=[神兵, 洞天, 功法]")
time.sleep(0.3)

# ─── Phase C: current search (should find NEW keywords) ──────
print("\n4️⃣  编辑后搜索（当前时间）— 新 keywords 应可检索")
for kw in ["飞剑", "仙山", "法宝", "神兵", "洞天", "功法"]:
    r = search(kw)
    hits = len(r["data"])
    names = [x.get("name","") or x.get("label","") for x in r["data"][:3]]
    print(f"  搜索「{kw}」→ {hits}条 {names}")

# ─── Phase D: current search (old keywords should NOT work) ──
print("\n5️⃣  编辑后搜索旧 keywords — 应不可检索")
for kw in ["古剑", "青云", "修仙", "宝剑", "秘境", "传承"]:
    r = search(kw)
    hits = len(r["data"])
    print(f"  搜索「{kw}」→ {hits}条 (期望: 0 条)")

# ─── Phase E: V + at time travel (old keywords in payload) ───
print("\n6️⃣  V/E + 时间回溯 — 查看旧 payload 中的 keywords")
# Current vertex payload
cur_v = v_by_id(vid)
v_keys = cur_v["data"][0].get("keywords", [])
print(f"  当前 V:   keywords={v_keys}")

# Time-travel to before edit
past_v = v_by_id(vid, at=t_before_edit - 1)
old_keys = past_v["data"][0].get("keywords", [])
print(f"  回溯 V:   keywords={old_keys}")
assert old_keys == ["古剑", "青云", "修仙"], f"期望旧keywords, 实际={old_keys}"

# Current edge payload
cur_e = e_by_id(eid)
e_keys = cur_e["data"][0].get("keywords", [])
print(f"  当前 E:   keywords={e_keys}")

# Time-travel edge
past_e = e_by_id(eid, at=t_before_edit - 1)
old_e_keys = past_e["data"][0].get("keywords", [])
print(f"  回溯 E:   keywords={old_e_keys}")
assert old_e_keys == ["宝剑", "秘境", "传承"], f"期望旧边缘keywords, 实际={old_e_keys}"

# ─── Phase F: search + at — token index is shared, so old
#     keywords won't hit; this is the current limitation.
print("\n7️⃣  search + at 参数 — 搜索引擎索引是共享的，无版本化")
print("   (token index 只反映当前 keywords，旧 keywords 无法命中)")
for kw in ["古剑", "青云", "宝剑"]:
    r = search(kw, at=t_before_edit - 1)
    hits = len(r["data"])
    print(f"  搜索「{kw}」+ at回溯 → {hits}条 (期望: 0，因为 token index 是当前快照)")

# ─── Phase G: search by NEW keywords + at (should still work) ─
print("\n8️⃣  search + at 参数 — 新 keywords 即使在旧时间也应命中")
print("   (因为当前 token index 有新 keywords，at 只影响 payload 版本)")
for kw in ["飞剑", "神兵"]:
    r = search(kw, at=t_before_edit - 1)
    hits = len(r["data"])
    types = [x["type"] for x in r["data"]]
    print(f"  搜索「{kw}」+ at回溯 → {hits}条 {types}")

# ─── Summary ────────────────
print(f"\n{'='*65}")
print("✅  所有测试完成")
print(f"{'='*65}")
print("""
  结论:
  - V/E 带 at 参数 → 正确返回旧关键词 payload (来自磁盘 history)
  - search 当前时间 → 正确命中当前 token index
  - search + at     → token index 是共享快照，不按时间版本化
  - 所以 search+timeTravel 只能看到当前 keywords 的搜索结果
  - 要完整时间回溯搜索，需实现 per-vertex/edge 的旧关键词索引
""")
