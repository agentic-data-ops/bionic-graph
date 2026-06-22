import json, urllib.request, time, sys

BASE = "http://127.0.0.1:8080"
GRAPH = "graph1"

# Step 0: Read test file
with open('.reasonix/output/fanrenxiuxianzhuan-brief-intro.md', 'r') as f:
    content = f.read()
print(f"Document: {len(content)} chars")

# Step 1: Create the graph document
data = json.dumps({
    "title": "凡人修仙传完整测试",
    "content": content,
    "tags": ["凡人修仙传", "测试"],
    "graph_name": GRAPH
}).encode()
req = urllib.request.Request(f"{BASE}/documents", data=data, headers={"Content-Type": "application/json"})
doc = json.loads(urllib.request.urlopen(req).read())
doc_id = doc["id"]
print(f"Document saved: {doc_id}")

# Step 2: Start extraction via backend task
req = urllib.request.Request(f"{BASE}/documents/{doc_id}/extract", method="POST")
task = json.loads(urllib.request.urlopen(req).read())
task_id = task["task_id"]
print(f"Task started: {task_id}")

# Step 3: Poll until complete
for i in range(30):
    time.sleep(2)
    req = urllib.request.urlopen(f"{BASE}/extract/tasks/{task_id}")
    status = json.loads(req.read())
    s = status["status"]
    steps = status.get("steps", [])
    for step in steps:
        d = step.get("detail") or ""
        icon = {"completed": "OK", "running": "..", "failed": "XX", "pending": "--"}.get(step["status"], "??")
        if step["status"] in ("running", "completed", "failed"):
            print(f"  {icon} [{step['status']:>9}] {step['label']} {step.get('progress_pct',0):.0f}% {d[:70]}")
    if s in ("completed", "failed"):
        if status.get("stats"):
            st = status["stats"]
            print(f"\nResults: {st.get('total_entities',0)} entities, {st.get('total_relations',0)} relations, {st.get('new_vertices',0)} vertices, {st.get('new_edges',0)} edges")
        if status.get("error"):
            print(f"\nError: {status['error'][:200]}")
        break
    print(f"--- Poll {i+1}: {s} ---")
else:
    print("Timeout")

# Step 4: Search "韩立" in graph1
print("\n--- Searching '韩立' in graph1 ---")
data = json.dumps({"query": "韩立"}).encode()
req = urllib.request.Request(f"{BASE}/search", data=data, headers={
    "Content-Type": "application/json",
    "X-Graph-Name": GRAPH
})
results = json.loads(urllib.request.urlopen(req).read())
print(f"Search returned {len(results['data'])} results")
for r in results['data'][:5]:
    print(json.dumps(r, ensure_ascii=False, indent=2))
