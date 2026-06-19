# Time Travel 实现对比: Bionic-Graph vs. Iceberg vs. Lance

---

## Apache Iceberg

**核心机制**: 快照链 (Snapshot isolation) — 表级文件清单

```
Write ─→ Manifest List v3 ─→ Manifest Files ─→ Data Files (Parquet)
                                    ↑
                        Manifest v2 ─┤
                        Manifest v1 ─┘

Time travel: SELECT * FROM table FOR SYSTEM_TIME AS OF '2024-06-10'
→ 读取 v2 的 Manifest List → 只扫描 v2 引用的 Data Files
```

| 特性 | 说明 |
|------|------|
| **粒度** | **表级** — 整个表一个快照 |
| **存储** | 不可变 Data Files + 可变 Manifest 元数据 |
| **读时** | 快照 = 完整的文件列表，读旧版本只需换一个 Manifest List |
| **写时** | 每次 commit 产生新 Manifest List（O(文件数) 元数据开销）|
| **删除** | 逻辑删除（删除文件在 Manifest 中取消引用），物理 compaction 回收 |
| **版本膨胀** | 需要定期 `expire_snapshots` 清理旧快照 |

**一句话**: Iceberg 的 time travel 是 **文件清单级** — 每次写入拍一张"哪些文件属于这个版本"的照片，读旧版本就是读旧照片。

---

## Lance / LanceDB

**核心机制**: 版本化 Log-Structured Merge (列式片段 + 版本索引)

```
version 3: Fragment list [F1, F2, F3]
version 2: Fragment list [F1, F2]  
version 1: Fragment list [F1]

Fragments 不可变。更新 = 新 fragment + deletion vector。

Time travel: table.to_latest() / table.at(version=2)
→ 读取 version 2 的 manifest → 只扫描 F1, F2
```

| 特性 | 说明 |
|------|------|
| **粒度** | **表级版本** — 整个数据集一个版本号 |
| **存储** | 不可变列式片段 (fragments) + deletion vectors |
| **读时** | 查版本 manifest → 确定需扫描的 fragment 列表 |
| **写时** | 追加新 fragment + 更新 manifest（O(1) 元数据）|
| **删除** | Deletion vector（位图标记已删除行），compaction 合并 |
| **版本膨胀** | 需要 compaction 合并 fragments |

**一句话**: Lance 的 time travel 是 **片段清单级** — 类似 Iceberg 但使用 LSM 树追加模式，写更轻量。

---

## Bionic-Graph

**核心机制**: 逐行 MVCC (per-vertex/edge 版本历史)

```
Vertex #42:
  _version: 3
  _updated_at: t3
  properties: { name: "Alicia" }
  _history: [
    { version: 1, updated_at: t1, properties: { name: "Alice" }},
    { version: 2, updated_at: t2, properties: { name: "Ali"   }}
  ]

Time travel: at_time(t1_5)
→ 遍历 _history，找到 t1 < t1_5 < t2 → 返回 version 1 的快照
```

| 特性 | 说明 |
|------|------|
| **粒度** | **记录级** — 每个 Vertex/Edge 独立版本历史 |
| **存储** | 历史快照内联在 Vertex/Edge 结构体中 |
| **读时** | 遍历 `_history` 找到对应时间点 → O(log n) 二分可优化 |
| **写时** | 推一个 `VersionRecord` 到 `_history` 数组 → O(1) |
| **删除** | `_is_deleted = true`，软删除，历史保留 |
| **版本膨胀** | 每个 Vertex 的 `_history` 随更新次数增长，无全局 compaction |

---

## 核心差异对比

| 维度 | **Iceberg** | **Lance** | **Bionic-Graph** |
|------|------------|-----------|-----------------|
| **快照粒度** | 表级 (整个表) | 表级 (整个数据集) | 记录级 (每个 Vertex/Edge) |
| **版本存储** | 外部 Manifest 文件 | 外部 Manifest + Fragments | 内联在 Vertex/Edge 结构体中 |
| **读旧版本成本** | O(快照数) 找 Manifest → O(文件数) 扫描 | O(1) 查 manifest → O(fragment数) 扫描 | O(历史长度) 遍历 `_history` |
| **写新版本成本** | O(文件数) 写 Manifest List | O(1) 追加 fragment | O(1) 推入 `_history` |
| **空间放大** | 元数据随文件数增长 | 随写入次数增长 (fragments) | 随每个 Vertex 的更新次数增长 |
| **适用范围** | 分析型数仓 (OLAP) | 向量+结构化混合 | 图数据库 (OLTP-like) |
| **混合时间点图遍历** | ❌ 只能查整表快照 | ❌ 只能查整表快照 | ✅ 每条路径独立回退 |

## 使用场景差异

```sql
-- Iceberg: 整表回退到时间点
SELECT * FROM table FOR SYSTEM_TIME AS OF '2024-06-10';

-- Bionic-Graph: 逐顶点回退的图遍历
{"step": "timeTravel", "at": "2024-06-10T12:00:00Z"},
{"step": "V"},
{"step": "out", "label": "knows", "depth": 3}
-- 每个 reachable 的顶点独立回退到该时间点的状态
```

## 总结

```
Iceberg/Lance = 「拍集体照」— 每次给整个数据集拍一张快照，便宜但只能整表回退
Bionic-Graph  = 「每人一本日记」— 每个顶点自己记变更历史，灵活但存储膨胀
```

Bionic-Graph 的实现更适合图场景的 time travel，因为图遍历天然需要 **逐元素** 的时间点回退。后续可改进方向：

1. **max_history 裁剪** — 限制每个 vertex 保留最近 N 个版本
2. **历史 offload** — 将旧 `VersionRecord` 移到独立的版本日志文件（类似 Iceberg 的 Manifest）
3. **二分查找优化** — `_history` 按时间有序，`at_time()` 可用二分替代线性扫描
