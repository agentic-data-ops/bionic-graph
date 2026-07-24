"""
Bionic-Graph 批量加载性能测试脚本
- 测试 1000 顶点 + ~5000 边的加载速度
- 分别测试 update_existing=True 和 update_existing=False 两种场景
- 每种场景测试首次加载（fresh）和重复加载（update/skip）两个阶段
- 报告输出到 .reasonix/output/
"""

import random
import time
import json
import os
import argparse
from datetime import datetime
from bionic_graph import Client

# ---------- 配置 ----------
BASE_URL = "http://7.225.183.194:8080"
NUM_VERTICES = 1000
TOTAL_EDGES = 5000
BATCH_SIZE = 500
SEED = 42
OUTPUT_DIR = os.path.join(os.path.dirname(__file__), "..", "output")

# 社会关系类型及权重范围
RELATIONSHIP_TYPES = [
    ("friend",    0.3, 0.9),
    ("colleague", 0.4, 0.7),
    ("family",    0.6, 0.95),
    ("neighbor",  0.2, 0.5),
    ("classmate", 0.3, 0.6),
    ("spouse",    0.8, 1.0),
    ("mentor",    0.5, 0.8),
]

LAST_NAMES = [
    "Wang", "Li", "Zhang", "Liu", "Chen", "Yang", "Zhao", "Huang",
    "Zhou", "Wu", "Xu", "Sun", "Hu", "Zhu", "Gao", "Lin", "He",
    "Guo", "Ma", "Luo", "Liang", "Song", "Zheng", "Xie", "Han",
    "Tang", "Feng", "Cao", "Xu", "Deng", "Xiao",
]
FIRST_NAMES = [
    "Wei", "Fang", "Lei", "Jie", "Yong", "Jun", "Tao", "Ming",
    "Liang", "Hao", "Xin", "Lina", "Yan", "Feng", "Qiang", "Bin",
    "Peng", "Hui", "Jian", "Chao", "Li", "Na", "Ying", "Xia",
    "Chen", "Long", "Wen", "Yang", "Kai", "Rui",
]

OCCUPATIONS = [
    "Engineer", "Doctor", "Teacher", "Lawyer", "Artist", "Writer",
    "Chef", "Scientist", "Accountant", "Designer", "Musician",
    "Architect", "Nurse", "Professor", "Journalist", "Pilot",
]

CITIES = [
    "Beijing", "Shanghai", "Guangzhou", "Shenzhen", "Hangzhou",
    "Chengdu", "Wuhan", "Nanjing", "Xi'an", "Chongqing",
]


def generate_person(idx):
    """生成一个随机人物顶点"""
    last = random.choice(LAST_NAMES)
    first = random.choice(FIRST_NAMES)
    return {
        "name": f"{last} {first}_{idx}",
        "labels": ["person"],
        "properties": {
            "occupation": random.choice(OCCUPATIONS),
            "city": random.choice(CITIES),
            "age": random.randint(18, 75),
            "idx": idx,
        },
    }


def generate_edges(vertex_names, total_edges):
    """基于顶点名称生成指定数量的边"""
    edges = []
    degree_count = {name: 0 for name in vertex_names}

    for _ in range(total_edges):
        if random.random() < 0.7 and any(d > 0 for d in degree_count.values()):
            weights = [degree_count[n] + 1 for n in vertex_names]
            source = random.choices(vertex_names, weights=weights, k=1)[0]
        else:
            source = random.choice(vertex_names)

        target = random.choice(vertex_names)
        if source == target:
            continue

        rel_type, min_s, max_s = random.choice(RELATIONSHIP_TYPES)
        strength = round(random.uniform(min_s, max_s), 2)

        edges.append({
            "source": source,
            "target": target,
            "name": rel_type,
            "strength": strength,
        })
        degree_count[source] += 1
        degree_count[target] += 1

    return edges


def batch_load_all(client, vertices, edges, batch_size, graph, update_existing):
    """执行完整的分批加载，返回计时和统计信息"""
    total_v = len(vertices)
    total_e = len(edges)
    v_created = v_updated = v_skipped = 0
    e_created = e_updated = e_skipped = 0
    batch_times = []
    t_start = time.time()

    # 分批加载顶点
    for i in range(0, total_v, batch_size):
        batch = vertices[i:i + batch_size]
        t0 = time.time()
        try:
            result = client.batch_load(
                entities=batch, relations=[], graph=graph,
                update_existing=update_existing,
            )
            v_created += result.get("vertices_created", 0)
            v_updated += result.get("vertices_updated", 0)
            v_skipped += result.get("vertices_skipped", 0)
        except Exception as e:
            print(f"    [ERROR] Vertex batch {i//batch_size + 1}: {e}")
            v_skipped += len(batch)
        elapsed = time.time() - t0
        batch_times.append(("vertex", i // batch_size + 1, elapsed, len(batch)))

    # 分批加载边
    for i in range(0, total_e, batch_size):
        batch = edges[i:i + batch_size]
        t0 = time.time()
        try:
            result = client.batch_load(
                entities=[], relations=batch, graph=graph,
                update_existing=update_existing,
            )
            e_created += result.get("edges_created", 0)
            e_updated += result.get("edges_updated", 0)
            e_skipped += result.get("edges_skipped", 0)
        except Exception as e:
            print(f"    [ERROR] Edge batch {i//batch_size + 1}: {e}")
            e_skipped += len(batch)
        elapsed = time.time() - t0
        batch_times.append(("edge", i // batch_size + 1, elapsed, len(batch)))

    total_elapsed = time.time() - t_start
    return {
        "total_elapsed": total_elapsed,
        "vertices_created": v_created,
        "vertices_updated": v_updated,
        "vertices_skipped": v_skipped,
        "edges_created": e_created,
        "edges_updated": e_updated,
        "edges_skipped": e_skipped,
        "batch_times": batch_times,
    }


def run_scenario(client, graph_name, update_existing, vertices, edges, batch_size):
    """运行一个测试场景：首次加载 + 重复加载"""
    print(f"\n{'='*60}")
    print(f"  场景: update_existing = {update_existing}")
    print(f"  图:   {graph_name}")
    print(f"{'='*60}")

    # 清理旧图
    try:
        client.delete_graph(graph_name)
        print(f"  [clean] 删除旧图")
    except Exception:
        pass
    client.create_graph(graph_name, description=f"Batch load perf test update_existing={update_existing}")
    print(f"  [setup] 创建新图")

    # ---- 阶段 1: 首次加载 ----
    print(f"\n  --- 阶段 1: 首次加载 (fresh) ---")
    r1 = batch_load_all(client, vertices, edges, batch_size, graph_name, update_existing)
    v_rate = NUM_VERTICES / (r1["total_elapsed"] if r1["total_elapsed"] > 0 else 1)
    print(f"  [done] 总耗时 {r1['total_elapsed']:.2f}s")
    print(f"    顶点: 创建={r1['vertices_created']} 更新={r1['vertices_updated']} 跳过={r1['vertices_skipped']}")
    print(f"    边:   创建={r1['edges_created']} 更新={r1['edges_updated']} 跳过={r1['edges_skipped']}")

    # ---- 阶段 2: 重复加载（相同数据） ----
    print(f"\n  --- 阶段 2: 重复加载 (same data, update={update_existing}) ---")
    r2 = batch_load_all(client, vertices, edges, batch_size, graph_name, update_existing)
    print(f"  [done] 总耗时 {r2['total_elapsed']:.2f}s")
    print(f"    顶点: 创建={r2['vertices_created']} 更新={r2['vertices_updated']} 跳过={r2['vertices_skipped']}")
    print(f"    边:   创建={r2['edges_created']} 更新={r2['edges_updated']} 跳过={r2['edges_skipped']}")

    # ---- 验证 ----
    print(f"\n  --- 验证 ---")
    try:
        v = client.execute_gremlin([{"step": "V"}, {"step": "count"}], graph=graph_name)
        print(f"    顶点数: {v.data}")
    except Exception as e:
        print(f"    V count 失败: {e}")
    try:
        e = client.execute_gremlin([{"step": "E"}, {"step": "count"}], graph=graph_name)
        print(f"    边数:   {e.data}")
    except Exception as e:
        print(f"    E count 失败: {e}")

    return {"phase1": r1, "phase2": r2}


def generate_report(results, output_path):
    """生成 Markdown 报告"""
    now = datetime.now().strftime("%Y-%m-%d %H:%M:%S")
    lines = []
    lines.append(f"# Bionic-Graph 批量加载性能测试报告")
    lines.append(f"")
    lines.append(f"- **测试时间**: {now}")
    lines.append(f"- **顶点数**: {NUM_VERTICES}")
    lines.append(f"- **边数**: ~{TOTAL_EDGES}")
    lines.append(f"- **批次大小**: {BATCH_SIZE}")
    lines.append(f"- **服务地址**: {BASE_URL}")
    lines.append(f"")
    lines.append(f"## 场景对比")
    lines.append(f"")
    lines.append(f"| 场景 | 阶段 | 总耗时 | 顶点创建 | 顶点更新 | 顶点跳过 | 边创建 | 边更新 | 边跳过 |")
    lines.append(f"|------|------|--------|----------|----------|----------|--------|--------|--------|")

    for label, key in [("update_existing=True", "true"), ("update_existing=False", "false")]:
        r = results[key]
        for phase, phase_label in [("phase1", "首次加载"), ("phase2", "重复加载")]:
            p = r[phase]
            lines.append(
                f"| {label} | {phase_label} | {p['total_elapsed']:.2f}s | "
                f"{p['vertices_created']} | {p['vertices_updated']} | {p['vertices_skipped']} | "
                f"{p['edges_created']} | {p['edges_updated']} | {p['edges_skipped']} |"
            )

    lines.append(f"")
    lines.append(f"## 详细批次耗时")
    lines.append(f"")
    for label, key in [("update_existing=True", "true"), ("update_existing=False", "false")]:
        lines.append(f"### {label}")
        lines.append(f"")
        lines.append(f"| 阶段 | 类型 | 批次 | 耗时 (s) | 数量 |")
        lines.append(f"|------|------|------|----------|------|")
        for phase, phase_label in [("phase1", "首次加载"), ("phase2", "重复加载")]:
            for typ, batch_no, elapsed, count in results[key][phase]["batch_times"]:
                lines.append(f"| {phase_label} | {typ} | {batch_no} | {elapsed:.3f} | {count} |")
        lines.append(f"")

    report = "\n".join(lines) + "\n"
    with open(output_path, "w") as f:
        f.write(report)
    print(f"\n报告已保存: {output_path}")
    return report


def main():
    parser = argparse.ArgumentParser(description="Bionic-Graph batch load performance test")
    parser.add_argument("--base-url", default=BASE_URL, help="Server base URL")
    parser.add_argument("--vertices", type=int, default=NUM_VERTICES, help="Number of vertices")
    parser.add_argument("--edges", type=int, default=TOTAL_EDGES, help="Number of edges")
    parser.add_argument("--batch-size", type=int, default=BATCH_SIZE, help="Batch size")
    parser.add_argument("--no-cleanup", action="store_true", help="Skip graph cleanup after test")
    args = parser.parse_args()

    random.seed(SEED)

    # 生成统一的测试数据（两种场景使用相同数据）
    print("生成测试数据 ...")
    vertices = [generate_person(i) for i in range(args.vertices)]
    vertex_names = [v["name"] for v in vertices]
    edges = generate_edges(vertex_names, args.edges)
    print(f"  顶点: {len(vertices)}")
    print(f"  边:   {len(edges)}")

    client = Client(base_url=args.base_url, timeout=120)
    health = client.health()
    print(f"服务状态: {health}")

    # 运行两个场景
    results = {}
    results["true"] = run_scenario(
        client, "stress_perf_true", True, vertices, edges, BATCH_SIZE
    )
    results["false"] = run_scenario(
        client, "stress_perf_false", False, vertices, edges, BATCH_SIZE
    )

    # 清理测试图
    if not args.no_cleanup:
        print(f"\n{'='*60}")
        print("  清理测试图 ...")
        for g in ["stress_perf_true", "stress_perf_false"]:
            try:
                client.delete_graph(g)
                print(f"  已删除: {g}")
            except Exception as e:
                print(f"  删除失败 {g}: {e}")
    else:
        print(f"\n跳过清理，测试图保留: stress_perf_true, stress_perf_false")

    # 输出报告
    output_dir = os.path.abspath(OUTPUT_DIR)
    os.makedirs(output_dir, exist_ok=True)
    report_path = os.path.join(output_dir, "batch_load_perf_report.md")
    report = generate_report(results, report_path)
    print(report)


if __name__ == "__main__":
    main()
