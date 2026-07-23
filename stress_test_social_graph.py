"""
Bionic-Graph 社会关系图谱压力测试脚本
- 创建 10,000 个顶点（模拟社会人物）
- 创建约 50,000 条边（模拟社会关系）
- 使用 batch_load API 批量导入，支持分批提交和进度显示
"""

import random
import time
import argparse
from bionic_graph import Client

# ---------- 配置 ----------
BASE_URL = "http://127.0.0.1:8080"
GRAPH_NAME = "stress"
NUM_VERTICES = 10_000
EDGES_PER_VERTEX_AVG = 5      # 每个顶点平均边数
BATCH_SIZE = 500               # 每批提交的顶点/边数

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

# 人物姓氏和名字池
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
    """生成一个随机人物属性"""
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


def generate_edges(vertex_names, avg_degree):
    """基于顶点名称生成边，使用偏好连接（preferential attachment）模型"""
    edges = []
    total_vertices = len(vertex_names)
    total_edges = int(total_vertices * avg_degree / 2)
    degree_count = {name: 0 for name in vertex_names}

    for _ in range(total_edges):
        # 70% 偏好连接，30% 随机连接
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


def batch_create_vertices(client, vertices, batch_size, graph=None):
    """使用 batch_load 分批创建顶点"""
    total = len(vertices)
    created = 0
    updated = 0
    skipped = 0
    t0 = time.time()

    for i in range(0, total, batch_size):
        batch = vertices[i:i + batch_size]
        try:
            result = client.batch_load(entities=batch, relations=[], graph=graph)
            created += result.get("vertices_created", 0)
            updated += result.get("vertices_updated", 0)
            skipped += result.get("vertices_skipped", 0)
        except Exception as e:
            print(f"  Batch {i//batch_size + 1} failed: {e}")
            skipped += len(batch)
        done = min(i + batch_size, total)
        print(f"  Vertices: {done}/{total} (created={created}, updated={updated}, skipped={skipped})")

    elapsed = time.time() - t0
    print(f"  Vertices done in {elapsed:.1f}s ({total/elapsed:.0f} verts/sec)")
    return created, updated, skipped


def batch_create_edges(client, edges, batch_size, graph=None):
    """使用 batch_load 分批创建边"""
    total = len(edges)
    created = 0
    updated = 0
    skipped = 0
    t0 = time.time()

    for i in range(0, total, batch_size):
        batch = edges[i:i + batch_size]
        try:
            result = client.batch_load(entities=[], relations=batch, graph=graph)
            created += result.get("edges_created", 0)
            updated += result.get("edges_updated", 0)
            skipped += result.get("edges_skipped", 0)
        except Exception as e:
            print(f"  Batch {i//batch_size + 1} failed: {e}")
            skipped += len(batch)
        done = min(i + batch_size, total)
        if done % 1000 == 0 or done == total:
            print(f"  Edges: {done}/{total} (created={created}, updated={updated}, skipped={skipped})")

    elapsed = time.time() - t0
    print(f"  Edges done in {elapsed:.1f}s ({total/elapsed:.0f} edges/sec)")
    return created, updated, skipped


def verify_graph(client, graph):
    """验证图数据"""
    print("\n--- Verification ---")
    try:
        result = client.execute_gremlin(
            [{"step": "V"}, {"step": "count"}],
            graph=graph,
        )
        print(f"  Vertex count: {result.data}")
    except Exception as e:
        print(f"  Vertex count query failed: {e}")

    try:
        result = client.execute_gremlin(
            [{"step": "E"}, {"step": "count"}],
            graph=graph,
        )
        print(f"  Edge count: {result.data}")
    except Exception as e:
        print(f"  Edge count query failed: {e}")

    try:
        result = client.search("Stark", graph=graph, limit=5)
        print(f"  Search 'Stark' sample: {result.data}")
    except Exception as e:
        print(f"  Search query failed: {e}")


def main():
    parser = argparse.ArgumentParser(description="Bionic-Graph social graph stress test")
    parser.add_argument("--base-url", default=BASE_URL)
    parser.add_argument("--graph", default=GRAPH_NAME)
    parser.add_argument("--vertices", type=int, default=NUM_VERTICES)
    parser.add_argument("--avg-degree", type=int, default=EDGES_PER_VERTEX_AVG)
    parser.add_argument("--batch-size", type=int, default=BATCH_SIZE)
    parser.add_argument("--skip-create", action="store_true", help="Skip vertex/edge creation (verify only)")
    parser.add_argument("--seed", type=int, default=42, help="Random seed for reproducibility")
    args = parser.parse_args()

    random.seed(args.seed)

    client = Client(base_url=args.base_url)
    print(f"Connecting to {args.base_url} ...")
    health = client.health()
    print(f"Health: {health}")

    # 确保目标图是默认图（X-Graph-Name header 会导致连接重置，必须用默认图）
    graphs = client.list_graphs()
    graph_names = [g.name for g in graphs.graphs]
    if args.graph not in graph_names:
        print(f"Graph '{args.graph}' not found, creating ...")
        if graphs.default and graphs.default != args.graph:
            print(f"  Deleting old default graph '{graphs.default}' ...")
            client.delete_graph(graphs.default, force=True)
        client.create_graph(args.graph, description="Social graph stress test - 10K vertices")
        print(f"Graph '{args.graph}' created as default.")
    elif graphs.default != args.graph:
        print(f"Graph '{args.graph}' exists but not default. Switching default ...")
        if graphs.default:
            print(f"  Deleting old default graph '{graphs.default}' ...")
            client.delete_graph(graphs.default, force=True)
        client.delete_graph(args.graph, force=True)
        client.create_graph(args.graph, description="Social graph stress test - 10K vertices")
        print(f"Graph '{args.graph}' recreated as default.")
    else:
        print(f"Graph '{args.graph}' is the default graph.")

    # 不传 graph 参数，避免 X-Graph-Name header 导致连接重置
    graph_param = None

    if not args.skip_create:
        # 1. 生成顶点数据
        print(f"\n[1/3] Generating {args.vertices} vertices ...")
        vertices = [generate_person(i) for i in range(args.vertices)]

        # 2. 批量创建顶点
        print(f"[2/3] Creating vertices (batch={args.batch_size}) ...")
        v_created, v_updated, v_skipped = batch_create_vertices(
            client, vertices, args.batch_size, graph=graph_param
        )

        # 3. 生成并批量创建边
        vertex_names = [v["name"] for v in vertices]
        print(f"[3/3] Creating edges (avg_degree={args.avg_degree}) ...")
        edges = generate_edges(vertex_names, args.avg_degree)
        print(f"  Generated {len(edges)} edges")
        e_created, e_updated, e_skipped = batch_create_edges(
            client, edges, args.batch_size, graph=graph_param
        )

        print(f"\n--- Summary ---")
        print(f"  Vertices: created={v_created}, updated={v_updated}, skipped={v_skipped}")
        print(f"  Edges:    created={e_created}, updated={e_updated}, skipped={e_skipped}")
    else:
        print("\nSkipping creation, verify only.")

    verify_graph(client, graph_param)


if __name__ == "__main__":
    main()
