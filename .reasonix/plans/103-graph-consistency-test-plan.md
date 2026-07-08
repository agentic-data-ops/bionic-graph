# Graph Crash Consistency Test Plan

## Purpose

Verify that all vertex/edge CRUD operations survive process crashes (SIGKILL) through WAL replay + O_DIRECT + checkpoint mechanisms. Also verify that index and token changes are consistent after recovery.

## Infrastructure

- **Server**: `./target/debug/bionic-graph` listening on `127.0.0.1:8080`
- **Data directory**: `data/` — cleaned before each test
- **Crash method**: `kill -9` (SIGKILL, no clean shutdown)
- **Restart**: fresh server process, waits for `/health` ok
- **Verification**: curl + Gremlin query + Python JSON parsing

---

## Phase 0: Pre-test Bug Fixes

Fix WAL data inconsistencies found during code audit:

### 0.1 Edge WAL uses padded data (fix P0)

`create_edge` (crud.rs:129) and `update_edge` (crud.rs:345) store padded data in WAL, but `create_vertex` stores unpadded data. This causes replay to interpret `data_len` as the padded length, corrupting replayed data.

**Fix**: Change both to use unpadded `&serialized`.

### 0.2 Token update path missing WAL (fix P1)

`add_token()` update path (existing token, append new ref) writes new data chunks and updates the index record, but does NOT write a WAL entry. If the process crashes, the new TokenRef is lost.

**Fix**: Add `redo_log.append(OpType::TokenUpdate, ...)` after `update_token_record`.

### 0.3 Clean replay_entry (fix P2)

Re-enable explicit match arms for `TokenCreate`, `TokenUpdate`, `TokenDelete`, `TokenIndexUpdate`, `VertexIndexUpdate`, `EdgeIndexUpdate` in `replay_entry` so they're not silently swallowed by `_ => {}`.

---

## Phase 1: Basic CRUD Crash Consistency

### TC1: Create single vertex

**Steps**:
1. Clean data, start server
2. `POST /vertices {"name":"Alice","keywords":["engineer"],"labels":["person"],"properties":{"age":30}}`
3. SIGKILL
4. Restart, query `V`
5. **Verify**: Vertex exists: id=1, name="Alice", labels=["person"], keywords=["engineer"], properties.age=30

### TC2: Create single edge

**Steps**:
1. Clean data, start server
2. Create vertex A (id=1), vertex B (id=2)
3. `POST /edges {"source":1,"target":2,"label":"knows","strength":0.9}`
4. SIGKILL
5. Restart, query `E`
6. **Verify**: Edge exists: id=1, source=1, target=2, label="knows", strength=0.9

### TC3: Multiple vertices + edges

**Steps**:
1. Clean data, start server
2. Create vertices: A(id=1), B(id=2), C(id=3)
3. Create edges: A→B (knows), B→C (works_with)
4. SIGKILL
5. Restart
6. **Verify**: 3 vertices, 2 edges, all attributes correct

### TC4: Token search after create

**Steps**:
1. Clean data, start server
2. Create vertex with keywords: `{"name":"Alice","keywords":["engineer","manager"]}`
3. SIGKILL
4. Restart
5. `POST /gremlin {"steps":[{"step":"search","text":"engineer"}]}`
6. **Verify**: Search returns Alice

---

## Phase 2: Update Crash Consistency

### TC5: Update vertex name

**Steps**:
1. Clean, start, create vertex Alice
2. `PUT /vertices/1 {"name":"Alice Smith"}`
3. SIGKILL
4. Restart, query `V` for id=1
5. **Verify**: name="Alice Smith"

### TC6: Update vertex properties

**Steps**:
1. Clean, start, create vertex Alice with age=30
2. `PUT /vertices/1 {"properties":{"age":31}}`
3. SIGKILL
4. Restart, query `V` for id=1
5. **Verify**: properties.age=31

### TC7: Update edge label

**Steps**:
1. Clean, start, create vertices A,B + edge (knows)
2. `PUT /edges/1 {"label":"friend_of"}`
3. SIGKILL
4. Restart, query `E` for id=1
5. **Verify**: label="friend_of"

### TC8: Token search after update

**Steps**:
1. Clean, start, create vertex Alice with keywords=["engineer"]
2. `PUT /vertices/1 {"keywords":["manager"]}`
3. SIGKILL
4. Restart
5. Search "manager" → finds Alice
6. Search "engineer" → may or may not find (token refs are appended, not replaced)
7. **Verify**: "manager" search returns Alice; search does not crash

---

## Phase 3: Delete Crash Consistency

### TC9: Soft delete vertex

**Steps**:
1. Clean, start, create Alice, Bob
2. `DELETE /vertices/1` (no force)
3. SIGKILL
4. Restart, query `V`
5. **Verify**: Only Bob visible (Alice soft-deleted)

### TC10: Soft delete edge

**Steps**:
1. Clean, start, create A, B, edge(1→2, knows)
2. `DELETE /edges/1` (no force)
3. SIGKILL
4. Restart, query `E`
5. **Verify**: No edges returned

### TC11: Hard delete vertex

**Steps**:
1. Clean, start, create Alice, Bob
2. `DELETE /vertices/1?force=true`
3. SIGKILL
4. Restart, query `V`
5. **Verify**: Only Bob visible

### TC12: Hard delete edge

**Steps**:
1. Clean, start, create A, B, edge(1→2, knows)
2. `DELETE /edges/1?force=true`
3. SIGKILL
4. Restart, query `E`
5. **Verify**: No edges returned

---

## Phase 4: Token Ref Cleanup Consistency

### TC13: Token ref cleanup on hard delete

**Steps**:
1. Clean, start, create Alice with keywords=["engineer"]
2. Search "engineer" → finds Alice (verify)
3. `DELETE /vertices/1?force=true`
4. SIGKILL
5. Restart
6. Search "engineer"
7. **Verify**: 0 results (token ref was cleaned up via `remove_entity_token_refs`)

### TC14: Token ref on update (new keyword)

**Steps**:
1. Clean, start, create Alice with keywords=["engineer"]
2. Search "engineer" → Alice
3. `PUT /vertices/1 {"keywords":["engineer","manager"]}`
4. SIGKILL
5. Restart
6. Search "engineer" → Alice (original keyword still works)
7. Search "manager" → Alice (new keyword works)
8. **Verify**: Both searches return Alice

---

## Phase 5: Multi-cycle Crash

### TC15: Create → Crash → Update → Crash → Delete → Crash

**Steps**:
1. Clean, start
2. Create Alice, Bob, edge A→B
3. SIGKILL
4. Restart → verify 2 vertices, 1 edge
5. Update Alice name to "Alice Smith"
6. SIGKILL
7. Restart → verify name="Alice Smith"
8. Hard delete Bob
9. SIGKILL
10. Restart → verify 1 vertex (Alice Smith), 0 edges
11. **Verify**: Final state correct after 3 crash cycles

---

## Phase 6: WAL Rotation

### TC16: Large writes trigger WAL rotation

**Steps**:
1. Clean, start
2. Write ~65MB of vertex data (creates enough WAL to trigger 64MB rotation)
3. SIGKILL after rotation
4. Restart
5. **Verify**: All vertices recovered (WAL replay reads from rotated files)

---

## Phase 7: Clean Shutdown

### TC17: SIGINT graceful shutdown

**Steps**:
1. Clean, start
2. Create Alice, Bob, edge
3. SIGINT (`kill -2`)
4. Wait for "All graphs flushed and checkpointed" log message
5. Restart
6. **Verify**: All data recovered (checkpoint guarantees durability)

---

## Phase 8: Mixed Workload

### TC18: Concurrent CRUD + crash

**Steps**:
1. Clean, start
2. Rapidly create/update/delete vertices and edges (10 operations)
3. SIGKILL during operations
4. Restart
5. **Verify**: Server starts, all operations that completed before WAL append are recovered; no corruption

---

## Expected Results Matrix

| TC | Operation | After SIGKILL | Status |
|----|-----------|---------------|--------|
| 1 | Create vertex | ✅ Full data | |
| 2 | Create edge | ✅ Full data | |
| 3 | Multi create | ✅ All entities | |
| 4 | Token search | ✅ Correct results | |
| 5 | Update name | ✅ New name | |
| 6 | Update property | ✅ New value | |
| 7 | Update label | ✅ New label | |
| 8 | Update keyword search | ✅ Both old+new | |
| 9 | Soft delete vertex | ✅ Not visible | |
| 10 | Soft delete edge | ✅ Not visible | |
| 11 | Hard delete vertex | ✅ Gone | |
| 12 | Hard delete edge | ✅ Gone | |
| 13 | Token cleanup | ✅ No stale refs | |
| 14 | Token on update | ✅ New token works | |
| 15 | Multi-cycle crash | ✅ Final state | |
| 16 | WAL rotation | ✅ All recovered | |
| 17 | SIGINT shutdown | ✅ All recovered | |
| 18 | Mixed workload | ✅ No corruption | |

## Bug Fix Checklist (Phase 0)

| Fix | File | Lines | Status |
|-----|------|-------|--------|
| create_edge WAL unpadded | src/graph/crud.rs | 129 | |
| update_edge WAL unpadded | src/graph/crud.rs | 345 | |
| add_token update WAL | src/graph/crud.rs | ~748 | |
| replay_entry explicit arms | src/graph/crud.rs | ~507 | |
