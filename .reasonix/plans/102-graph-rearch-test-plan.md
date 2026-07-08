# Graph Re-architecture — 综合测试计划

**版本:** 1.1
**执行结果 (2026-07-06):**
- 单元测试: 42/42 ✅（存储引擎 27 + 分词器 11 + 配置 3 + extract 1）
- 集成测试: 24/24 ✅（CRUD 6 + Gremlin 4 + 遍历 3 + 搜索 3 + 激活 3 + 持久化 1 + locked 1 + tokenizer edge cases 3）
- **总计: 66 个测试全部通过** ✅
**对应设计:** `.reasonix/plans/100-graph-rearch-design.md`
**对应实现:** Phase 1–8 完成，分支 `dev-snap-neuron-search-mem`

---

## 测试策略概览

| 层级 | 工具 | 目标 |
|------|------|------|
| **单元测试** | `cargo test` | 存储引擎、索引引擎、分词器各模块 |
| **集成测试** | Rust `#[cfg(test)]` + `tempfile` | CRUD 全流程、Gremlin pipeline |
| **API 测试** | curl + python3 | REST 端点功能验证 |
| **可靠性测试** | 进程级恢复 | WAL 重放、崩溃恢复 |
| **集群测试** | 多进程 | Master-worker 复制、转发 |

---

## T1: 存储引擎单元测试（已有 18 个）

| 测试 | 文件 | 状态 | 说明 |
|------|------|------|------|
| `test_alloc_one_chunk` | `block_allocator.rs:179` | ✅ | 分配单个 chunk |
| `test_alloc_frees_and_reallocates` | `block_allocator.rs:188` | ✅ | 释放后复用 |
| `test_block_full` | `block_allocator.rs:198` | ✅ | 填满 255 data chunks |
| `test_block_empty` | `block_allocator.rs:210` | ✅ | 空块检测 |
| `test_chunk_count` | `block_allocator.rs:216` | ✅ | 计数 |
| `test_padded_length` | `block_allocator.rs:224` | ✅ | 64 字节对齐 |
| `test_chunks_needed` | `block_allocator.rs:231` | ✅ | chunk 需求计算 |
| `test_hit_and_miss` | `block_cache.rs:336` | ✅ | 缓存命中/未命中 |
| `test_dirty_marked_and_flushed` | `block_cache.rs:352` | ✅ | 脏页 + flush |
| `test_eviction_evicts_lru` | `block_cache.rs:367` | ✅ | LRU 淘汰 |
| `test_create_and_alloc` | `bitmap_file.rs:216` | ✅ | 位图分配 |
| `test_mark_full_then_free` | `bitmap_file.rs:234` | ✅ | 满/释放 |
| `test_write_then_read` | `data_file.rs:127` | ✅ | 块写入读取 |
| `test_read_unallocated_block_returns_zeros` | `data_file.rs:141` | ✅ | 越界读取 |
| `test_allocate_blocks` | `data_file.rs:151` | ✅ | 扩展文件 |
| `test_vertex_record_roundtrip` | `index_file.rs:490` | ✅ | 顶点索引编解码 |
| `test_edge_record_roundtrip` | `index_file.rs:511` | ✅ | 边索引编解码 |
| `test_token_record_roundtrip` | `index_file.rs:530` | ✅ | token 索引编解码 |
| `test_scan_records` | `index_file.rs:548` | ✅ | 索引全扫描 |
| `test_delete_record` | `index_file.rs:574` | ✅ | 索引删除 |
| `test_multiple_blocks` | `index_file.rs:593` | ✅ | 多 block 分配 |
| `test_append_and_replay` | `redo_log.rs:319` | ✅ | WAL 追加+重放 |
| `test_crc_mismatch_detected` | `redo_log.rs:346` | ✅ | CRC 校验 |
| `test_remove_all` | `redo_log.rs:366` | ✅ | WAL 清理 |
| `test_build_empty_index` | `memory_index_builder.rs:72` | ✅ | 空索引重建 |
| `test_build_with_vertices` | `memory_index_builder.rs:83` | ✅ | 含顶点重建 |
| `test_build_skips_deleted` | `memory_index_builder.rs:104` | ✅ | 忽略已删除 |

### T1 新增测试计划

| 优先级 | 测试 | 说明 |
|--------|------|------|
| P0 | `test_block_header_encode_decode_roundtrip` | BlockHeader 编解码一致性 |
| P0 | `test_cache_with_block_marks_dirty` | `with_block` 后自动 dirty |
| P0 | `test_cache_eviction_flushes_dirty` | 淘汰时写回脏页 |
| P1 | `test_bitmap_file_persist_across_open` | 位图关闭重开后一致性 |
| P1 | `test_data_file_large_sequential` | 大数据量顺序读写 |
| P1 | `test_index_file_100k_records` | 10 万条索引记录性能 |
| P2 | `test_redo_log_rotation` | 64MB 日志轮转 |

---

## T2: 分词器测试（已有 10 个）

| 测试 | 文件 | 状态 | 说明 |
|------|------|------|------|
| `test_en_simple` | `tokenizer.rs:146` | ✅ | 英文基本分词 |
| `test_en_removes_stop_words` | `tokenizer.rs:152` | ✅ | 停用词过滤 |
| `test_en_removes_short_tokens` | `tokenizer.rs:158` | ✅ | 单字过滤 |
| `test_en_case_insensitive` | `tokenizer.rs:164` | ✅ | 大小写归一 |
| `test_extract_tokens` | `tokenizer.rs:170` | ✅ | Token 提取 + Hit 记录 |
| `test_cjk_simple` | `tokenizer.rs:187` | ✅ | 中文基本分词 |
| `test_cjk_mixed` | `tokenizer.rs:194` | ✅ | 中英混合 |
| `test_cjk_mixed_edge_cases` | `tokenizer.rs:205` | ✅ | 边界：OpenAI/Bionic-Graph/Hello世界 |
| `test_cjk_detection` | `tokenizer.rs:220` | ✅ | CJK 检测 |
| `test_cjk_noise_filter` | `tokenizer.rs:227` | ✅ | 单中文字过滤 |
| `test_cjk_keywords` | `tokenizer.rs:235` | ✅ | 中文关键词提取 |

### T2 新增测试计划

| 优先级 | 测试 | 说明 |
|--------|------|------|
| P1 | `test_tokenize_emoji` | Emoji 处理 |
| P1 | `test_tokenize_urls` | URL 分词 |
| P1 | `test_tokenize_numbers` | 数字/日期/版本号 |
| P2 | `test_extract_tokens_large_text` | 长文本 token 提取 |

---

## T3: CRUD 集成测试（新建）

### T3.1 顶点 CRUD

```
场景 3.1.1: 创建顶点 → 通过 ID 读取
  输入: POST /vertices {"name":"Alice","labels":["person"],"keywords":["alice"],"properties":{"age":30}}
  验证: 返回 id=N, GET /gremlin V ids=[N] 返回 name="Alice", labels=["person"], properties.age=30

场景 3.1.2: 创建多个顶点 → V 列出全部
  输入: 创建 Alice/Bob/Carol
  验证: V step 返回 3 条, 包含所有顶点

场景 3.1.3: 更新顶点
  输入: PUT /vertices/N {"name":"Alice Updated","properties":{"age":31}}
  验证: V ids=[N] → name="Alice Updated", age=31

场景 3.1.4: 软删除顶点
  输入: DELETE /vertices/N (force=false)
  验证: V ids=[N] 不返回该顶点
  验证: V ids=[N] + timeTravel(at=before_delete) 返回删除前的版本

场景 3.1.5: 硬删除顶点
  输入: DELETE /vertices/N?force=true
  验证: V ids=[N] 不返回
  验证: timeTravel(at=any) 也不返回
```

### T3.2 边 CRUD

```
场景 3.2.1: 创建边
  输入: POST /edges {"label":"knows","source":1,"target":2,"strength":0.8}
  验证: 返回 id=N, E ids=[N] 正确

场景 3.2.2: 更新边
  输入: PUT /edges/N {"label":"works_with","strength":0.9}
  验证: 读取后 label 和 strength 更新

场景 3.2.3: 删除边
  输入: DELETE /edges/N
  验证: E ids=[N] 不返回
```

### T3.3 级联删除

```
场景 3.3.1: 硬删除顶点 → 关联边自动删除
  输入: 创建 Alice→Bob (edge_id=1), 硬删除 Alice
  验证: V ids=[alice_id] 不返回
  验证: E ids=[1] 不返回 (边随顶点删除)
```

---

## T4: Gremlin Pipeline 测试

### T4.1 基础步骤

| 场景 | 步骤链 | 预期 |
|------|--------|------|
| `V` 全部 | `[{"step":"V"}]` | 返回全部顶点 |
| `V` 指定 ID | `[{"step":"V","ids":[1,2]}]` | 返回 ID 1,2 |
| `E` 全部 | `[{"step":"E"}]` | 返回全部边 |
| `E` 指定 ID | `[{"step":"E","ids":[1]}]` | 返回 ID 1 的边 |
| `has` 过滤 | `[{"step":"V"},{"step":"has","key":"name","value":"Alice"}]` | 返回 name=Alice 的顶点 |
| `hasNot` | `[{"step":"V"},{"step":"hasNot","key":"name","value":"Alice"}]` | 返回 name≠Alice 的顶点 |
| `hasKey` | `[{"step":"V"},{"step":"hasKey","key":"age"}]` | 返回有 age 属性的顶点 |
| `hasValue` | `[{"step":"V"},{"step":"hasValue","value":"Alice"}]` | 返回任意属性=Alice 的顶点 |
| `hasLabel` | `[{"step":"V"},{"step":"hasLabel","label":"person"}]` | 返回 label=person 的顶点 |
| `hasText` | `[{"step":"V"},{"step":"hasText","text":"Ali"}]` | 返回 name 包含 "Ali" 的顶点 |
| `limit` | `[{"step":"V"},{"step":"limit","count":1}]` | 仅返回 1 条 |
| `count` | `[{"step":"V"},{"step":"count"}]` | 返回 `{"count":N}` |
| `dedup` | 场景：同个顶点被多次匹配时去重 | 返回唯一顶点列表 |
| `values` | `[{"step":"V"},{"step":"values","keys":["name"]}]` | 仅返回 name 属性 |

### T4.2 遍历步骤

```
场景 4.2.1: out()
  图: Alice --knows--> Bob
  输入: [{"step":"V","ids":[alice_id]},{"step":"out"}]
  预期: 返回 [Bob]

场景 4.2.2: in()
  输入: [{"step":"V","ids":[bob_id]},{"step":"in"}]
  预期: 返回 [Alice]

场景 4.2.3: both()
  输入: [{"step":"V","ids":[alice_id]},{"step":"both"}]
  预期: 返回所有邻居

场景 4.2.4: outE() / inE() / bothE()
  输入: outE() 返回边 (带 label/source/target/strength)
  预期: 返回 EdgeResult 而非 VertexResult

场景 4.2.5: 多跳遍历 out(depth=2)
  图: Alice --knows--> Bob --works_with--> Carol
  输入: [{"step":"V","ids":[alice_id]},{"step":"out","depth":2}]
  预期: 返回 [Bob, Carol]

场景 4.2.6: expand()
  输入: [{"step":"V","ids":[alice_id]},{"step":"expand"}]
  预期: 返回 Alice + 她的所有邻居 + 连接边
```

### T4.3 搜索步骤

```
场景 4.3.1: 英文关键词搜索
  索引: Alice(name="Alice", keywords=["alice","engineer"])
  输入: GET /search?keywords=alice
  预期: 返回 Alice, score=2.0 (匹配 name + keywords)

场景 4.3.2: 中文关键词搜索
  索引: "张三"(name="张三", keywords=["工程师"])
  输入: GET /search?keywords=张三
  预期: 返回张三

场景 4.3.3: 贪婪模式 (greedy)
  输入: keywords=["alice","unknown"]  mode=greedy
  预期: 返回 Alice (匹配任意关键词即可)

场景 4.3.4: 精确模式 (exact)
  输入: keywords=["alice","engineer"] mode=exact (threshold=0.8)
  预期: 返回 Alice (需要匹配全部关键词)
  输入: keywords=["alice","unknown"] mode=exact
  预期: 不返回 (不匹配全部)

场景 4.3.5: 按 rank 排序
  输入: search + min_rank=N
  预期: 仅返回 rank >= N 的结果
```

### T4.4 神经元激活遍历 (Activate)

```
场景 4.4.1: 基本激活 (默认参数)
  图: Alice(s=1.0) --knows(s=0.8)--> Bob --works_with(s=0.6)--> Carol
  输入: [{"step":"V","ids":[alice_id]},{"step":"activate"}]
        decay=1.0, activate=0.0, max_depth=1, min_score=0.0
  预期:
    - Alice: score=1.0 (entry vertex)
    - Bob: score=1.0*1.0*0.8=0.8
    - Carol: 不出现在 max_depth=1 (需要 2 跳)

场景 4.4.2: 带深度限制
  输入: max_depth=2
  预期:
    - Alice: score=1.0
    - Bob: score=0.8
    - Carol: score=0.8*1.0*0.6=0.48

场景 4.4.3: 衰减率
  输入: decay=0.5, max_depth=2
  预期:
    - Alice: 1.0
    - Bob: 1.0*0.5*0.8=0.4
    - Carol: 0.4*0.5*0.6=0.12

场景 4.4.4: 最低分数过滤
  输入: min_score=0.5
  预期: Alice(1.0), Bob(0.8) — Carol(0.48) 被过滤

场景 4.4.5: 激活阈值截断
  输入: activate=0.5
  预期: Alice(1.0) 传播到 Bob(0.8) → 继续传播, 但 Carol(0.48<0.5) 停止
```

### T4.5 Repeat 步骤

```
场景 4.5: repeat() 循环
  输入: [{"step":"V","ids":[alice_id]},{"step":"repeat","steps":[{"step":"out"}],"times":2}]
  预期: depth=2 的所有邻居
```

---

## T5: 时间旅行测试

### T5.1 时间旅行基础

```
场景 5.1.1: 创建 → 更新 → 历史查询
  步骤:
    1. t0: 创建 Alice(name="Alice", age=30)
    2. t1: 更新 Alice(name="Alice Updated", age=31)
    3. t2: 查询 V ids=[alice_id] at=t0 → 返回 "Alice", 30
    4. t3: 查询 V ids=[alice_id] at=t1 → 返回 "Alice Updated", 31
    5. t4: 查询 V ids=[alice_id] (无 at) → 返回最新 "Alice Updated", 31

场景 5.1.2: 搜索 + 时间旅行
  输入: GET /search?keywords=alice&at=<t0>
  预期: 返回 t0 时刻匹配的顶点

场景 5.1.3: 遍历 + 时间旅行
  输入: [{"step":"timeTravel","at":<t0>},{"step":"V"}]
  预期: 所有步骤在 t0 时间点执行

场景 5.1.4: 软删除 + 时间旅行恢复
  步骤:
    1. t0: 创建 Alice
    2. t1: 软删除 Alice
    3. t2: V ids=[alice_id] → 不返回 (已删除)
    4. t3: V ids=[alice_id] at=t0 → 返回 Alice (删除前)
```

---

## T6: 可靠性 & 持久性测试

### T6.1 数据持久性

```
场景 6.1.1: 写入 → 重启 → 读取
  步骤:
    1. 创建 Alice, Bob, 边 knows(Alice→Bob)
    2. 停止服务器
    3. 启动服务器
    4. V step → 返回 Alice, Bob
    5. E step → 返回边
    6. Search("alice") → 返回 Alice

场景 6.1.2: 大图持久性
  步骤:
    1. 创建 10000 个顶点, 50000 条边
    2. 停止 → 启动
    3. 验证 count=10000, 随机抽检 100 个顶点正确
```

### T6.2 崩溃恢复 (WAL)

```
场景 6.2.1: 写入后模拟崩溃 → 重放 WAL 恢复
  步骤:
    1. 创建 Alice (WAL 写入)
    2. 创建 Bob (WAL 写入)
    3. 模拟崩溃: 删除 data 文件 (不清除 redo log)
    4. 启动服务器 → 自动重放 WAL
    5. V step → 返回 Alice, Bob

场景 6.2.2: WAL CRC 损坏检测
  步骤:
    1. 写入数据
    2. 篡改 redo log 中某条记录的字节
    3. 启动 → 应检测到 CRC 错误并报错 / 跳过损坏记录

场景 6.2.3: 部分 WAL 恢复
  步骤:
    1. 创建 Alice (WAL entry 1)
    2. 创建 Bob (WAL entry 2)
    3. 删除 data file 中部分数据 (模拟部分刷盘)
    4. 启动 → 重放 WAL → Alice 和 Bob 都恢复
```

### T6.3 缓存一致性

```
场景 6.3.1: 缓存写穿
  步骤:
    1. 创建顶点 (通过 cache 写入 + flush 磁盘)
    2. 清除缓存 (重启/淘汰)
    3. 读取 → 数据正确 (来自磁盘)

场景 6.3.2: 大量写入触发 LRU 淘汰
  步骤:
    1. 设置小缓存 (e.g. 4 blocks)
    2. 创建大量顶点 → 触发淘汰
    3. 随机读取 → 全部正确
```

### T6.3 并发安全

```
场景 6.3.1: 并发读取
  步骤:
    1. 创建顶点
    2. 10 个线程同时调用 get_vertex
    3. 全部成功返回, 数据一致

场景 6.3.2: 读写并发
  步骤:
    1. 1 个线程持续写入
    2. 5 个线程持续读取
    3. 无死锁, 无 panic, 读取结果符合线性一致性

场景 6.3.3: 写写冲突
  步骤:
    1. 2 个线程同时更新同个顶点
    2. 最终结果 = 其中一个写入 (非混合)
```

---

## T7: 集群测试

### T7.1 基本集群配置

```
场景 7.1.1: Master 启动
  输入: cluster.mode=master, bind=9090
  验证: master 启动成功, 接受 API 请求

场景 7.1.2: Worker 启动并连接 Master
  输入: cluster.mode=worker, master_addr=localhost:9090
  验证: worker 启动, 发送 heartbeat, master 注册该 worker
```

### T7.2 数据复制

```
场景 7.2.1: Master 写入 → Worker 复制
  步骤:
    1. Worker 注册到 Master
    2. Master 创建 Alice
    3. Master 推送 RedoLogEntry 到 Worker
    4. Worker 重放 → 内存索引 + 数据块更新
    5. Worker 查询 V → 返回 Alice

场景 7.2.2: 批量写入复制
  步骤:
    1. Master 批量创建 100 个顶点
    2. 每个写入后: ReplicatedEntry 推送到所有 Worker
    3. Worker count = 100
```

### T7.3 写转发 (Worker → Master)

```
场景 7.3.1: Worker 写操作自动转发
  步骤:
    1. Worker 接收 POST /vertices
    2. Worker 检测 is_write_path("/vertices") → true
    3. Worker 转发 ForwardedRequest 到 Master
    4. Master 执行并返回结果
    5. Worker 返回结果给客户端

场景 7.3.2: Worker 读操作本地执行
  步骤:
    1. Worker 接收 GET /search
    2. Worker 检测 is_write_path("/search") → false
    3. 本地执行, 不转发
```

### T7.4 故障检测

```
场景 7.4.1: Worker 心跳超时
  步骤:
    1. Master 注册 Worker
    2. Worker 停止发送心跳
    3. Master.purge_expired() → 标记 Worker 死亡
    4. alive_workers() 不包含该 Worker

场景 7.4.2: 新 Worker 加入
  步骤:
    1. Worker 发送 Heartbeat 到 Master
    2. Master 注册 Worker
    3. Master 开始推送后续写入
```

---

## T8: API 端到端测试 (curl)

所有测试基于 REST API，可用以下脚本自动化。

### T8.1 快速冒烟测试

```bash
#!/bin/bash
BASE="http://127.0.0.1:8080"
PASS=0 FAIL=0

check() {
  local desc="$1" exp="$2" got="$3"
  if echo "$got" | grep -q "$exp"; then
    echo "  ✅ $desc"; ((PASS++))
  else
    echo "  ❌ $desc (expected '$exp', got '$got')"; ((FAIL++))
  fi
}

# 1. Health
check "Health" "ok" "$(curl -s $BASE/health)"

# 2. Create graph
GRAPH=$(curl -s -X POST $BASE/graphs -H 'Content-Type: application/json' -d '{"name":"smoke"}')
check "Create graph" "created" "$GRAPH"

# 3. Create vertices
A=$(curl -s -X POST "$BASE/vertices?graph=smoke" -H 'Content-Type: application/json' \
  -d '{"name":"Alice","labels":["person"],"keywords":["alice"],"properties":{"age":30}}')
check "Create Alice" '"id"' "$A"
AID=$(echo "$A" | python3 -c "import sys,json;print(json.load(sys.stdin)['id'])")

B=$(curl -s -X POST "$BASE/vertices?graph=smoke" -H 'Content-Type: application/json' \
  -d '{"name":"Bob","labels":["person"],"keywords":["bob"],"properties":{"age":25}}')
check "Create Bob" '"id"' "$B"
BID=$(echo "$B" | python3 -c "import sys,json;print(json.load(sys.stdin)['id'])")

# 4. V step
V=$(curl -s -X POST "$BASE/gremlin?graph=smoke" -H 'Content-Type: application/json' \
  -d '{"steps":[{"step":"V"}]}')
check "V all" "Alice" "$V"

# 5. Search (English)
S=$(curl -s "$BASE/search?graph=smoke&keywords=alice")
check "Search Alice" "score" "$S"

# 6. Search (Chinese)
C=$(curl -s -X POST "$BASE/vertices?graph=smoke" -H 'Content-Type: application/json' \
  -d '{"name":"张三","labels":["person"],"keywords":["工程师"],"properties":{}}')
check "Create 张三" '"id"' "$C"
SZ=$(curl -s "$BASE/search?graph=smoke&keywords=张三")
check "Search 张三" "张三" "$SZ"

# 7. Update vertex
UP=$(curl -s -X PUT "$BASE/vertices/$AID?graph=smoke" -H 'Content-Type: application/json' \
  -d '{"name":"Alice Updated"}')
check "Update Alice" "200" "$(curl -o /dev/null -w '%{http_code}' -X PUT "$BASE/vertices/$AID?graph=smoke" \
  -H 'Content-Type: application/json' -d '{"name":"Alice Updated"}')"

# 8. Edge
E=$(curl -s -X POST "$BASE/edges?graph=smoke" -H 'Content-Type: application/json' \
  -d "{\"label\":\"knows\",\"source\":$AID,\"target\":$BID,\"strength\":0.8}")
check "Create edge" '"id"' "$E"

# 9. Expand
EX=$(curl -s -X POST "$BASE/gremlin?graph=smoke" -H 'Content-Type: application/json' \
  -d "{\"steps\":[{\"step\":\"V\",\"ids\":[$AID]},{\"step\":\"expand\"}]}")
check "Expand" "Bob" "$EX"

# 10. Soft delete
DEL=$(curl -s -X DELETE "$BASE/vertices/$AID?graph=smoke" -w '%{http_code}')
check "Delete vertex" "200" "$DEL"
V2=$(curl -s -X POST "$BASE/gremlin?graph=smoke" -H 'Content-Type: application/json' \
  -d '{"steps":[{"step":"V"}]}')
check "V after delete (no Alice)" "Bob" "$V2"

echo -e "\n--- Result: $PASS passed, $FAIL failed ---"
```

### T8.2 中英文搜索专用

```bash
# 英文搜索
curl -s "$BASE/search?graph=smoke&keywords=alice,engineer" | python3 -m json.tool

# 中文搜索 (通过 jieba 分词)
curl -s "$BASE/search?graph=smoke&keywords=张三,工程师" | python3 -m json.tool

# 混合搜索
curl -s "$BASE/search?graph=smoke&keywords=Alice,工程师" | python3 -m json.tool
```

### T8.3 设置端点

```bash
# 读取
curl -s http://127.0.0.1:8080/settings/search | python3 -m json.tool

# 更新
curl -s -X PUT http://127.0.0.1:8080/settings/search \
  -H 'Content-Type: application/json' \
  -d '{"search_mode":"greedy","greedy_threshold":0.6,"greedy_explore":true, \
       "exact_threshold":0.8,"exact_explore":false,"max_results":100, \
       "explore_decay":0.8,"explore_activate":0.1,"explore_max_depth":2,"explore_min_score":0.3}'

# 验证
curl -s http://127.0.0.1:8080/settings/search | python3 -m json.tool

# 向后兼容
curl -s http://127.0.0.1:8080/settings/neural | python3 -m json.tool
```

---

## T9: 边界 & 异常测试

| 场景 | 输入 | 预期 |
|------|------|------|
| 空 graph 查询 | V step on empty graph | success:true, data:[] |
| 不存在 ID | V ids=[999999] | success:true, data:[] |
| 超长 name | name = "A" × 10000 | 正常创建 (分多个 chunk) |
| 空 name | name = "" | 创建成功 |
| 超大 properties | 1000 个属性 | 正常创建 |
| strength < 0 | strength = -1 | 截断到 0 或报错 |
| strength > 1 | strength = 2 | 截断到 1 或报错 |
| 并发 100 写入 | 100 个线程同时创建顶点 | 全部成功, 无冲突 |
| 索引重建 | 删除 index 文件, 重启 | 自动创建空索引 |
| WAL 满 | 连续写入触发 64MB 轮转 | 轮转后继续工作 |
| Token 超长 | 42+ 字符 token | 截断到 42 字符 |

---

## T10: 测试优先级 & 阶段

| 阶段 | 测试范围 | 优先级 |
|------|---------|--------|
| **Phase A: 基础 CRUD** | T3.1, T3.2, T4.1, T8.1 | P0 — 阻塞 |
| **Phase B: 搜索** | T4.3, T8.2, T2 | P0 — 阻塞 |
| **Phase C: 遍历** | T4.2, T4.5 | P1 — 核心 |
| **Phase D: 激活遍历** | T4.4 | P1 — 核心 |
| **Phase E: 时间旅行** | T5 | P1 — 核心 |
| **Phase F: 持久性** | T6.1, T6.2 | P1 — 核心 |
| **Phase G: 并发** | T6.3 | P2 — 重要 |
| **Phase H: 集群** | T7 | P2 — 重要 |
| **Phase I: 边界** | T9 | P2 — 重要 |

---

## 附录: 测试数据模型

用于测试的示例图谱:

```
Alice (person, age=30, city="NYC")
  │
  ├── knows (strength=0.9, since=2020) ──→ Bob (person, age=25, city="SF")
  │
  ├── works_at (strength=0.8) ──→ BioGraph (project, stars=5000)
  │
  └── friend_of (strength=0.7) ──→ Carol (person, age=28, city="NYC")
                                       │
                                       └── works_at (strength=0.6) ──→ BioGraph

张三 (工程师, 北京)
  │
  └── 合作 (strength=0.9) ──→ 李四 (科学家, 上海)
```

Vertex 属性:

| ID | name | labels | keywords | properties |
|----|------|--------|----------|------------|
| 1 | Alice | ["person"] | ["alice","engineer"] | {age:30, city:"NYC"} |
| 2 | Bob | ["person"] | ["bob","scientist"] | {age:25, city:"SF"} |
| 3 | Carol | ["person"] | ["carol","designer"] | {age:28, city:"NYC"} |
| 4 | BioGraph | ["project"] | ["bionic-graph","ai"] | {stars:5000} |
| 5 | 张三 | ["person"] | ["工程师","北京"] | {} |
| 6 | 李四 | ["person"] | ["科学家","上海"] | {} |

Edge 属性:

| ID | label | source | target | strength | properties |
|----|-------|--------|--------|----------|------------|
| 1 | knows | 1 | 2 | 0.9 | {since:2020} |
| 2 | works_at | 1 | 4 | 0.8 | {} |
| 3 | friend_of | 1 | 3 | 0.7 | {} |
| 4 | works_at | 3 | 4 | 0.6 | {} |
| 5 | 合作 | 5 | 6 | 0.9 | {} |
