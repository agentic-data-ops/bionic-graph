#!/usr/bin/env bash
# ============================================================================
# Bionic-Graph CLI 真实调用测试（v2 — 输出提取优化版）
# 覆盖全部 11 个主题、45 个动作
# 调用方式: cd sdk/python && bash tests/test_cli_real.sh
# ============================================================================
set +e

BASE_DIR="$(cd "$(dirname "$0")/.." && pwd)"
BASE_URL="${BASE_URL:-http://127.0.0.1:8080}"
CLI="python -m bionic_graph.cli --base-url ${BASE_URL}"
PASS=0; FAIL=0; FAILED_TESTS=""
TS=$(date +%s)
TEST_GRAPH="test_graph_${TS}"

run() {
    local desc="$1" expected_exit="$2"
    shift 2
    echo "─── ${desc} ───"
    cd "$BASE_DIR" >/dev/null
    local output
    output=$($CLI "$@" 2>&1)
    local rc=$?
    cd - >/dev/null
    echo "$output" | head -3 | sed 's/^/  > /'
    if [ "$rc" -eq "$expected_exit" ]; then
        ((PASS++))
        echo "  ✅ PASS"
    else
        ((FAIL++))
        FAILED_TESTS="${FAILED_TESTS}  - ${desc}\n"
        echo "  ❌ FAIL (exit=${rc}, expected=${expected_exit})"
    fi
    # Save to global for ID extraction
    LAST_OUTPUT="$output"
    return $rc
}

save_id() {
    # Extract first "id: <value>" from LAST_OUTPUT
    ID_VAL=$(echo "$LAST_OUTPUT" | grep -oP '^id:\s*\K(.+)$' | head -1 | tr -d ' ')
    echo "  → ID: ${ID_VAL}"
}

save_json_id() {
    # Extract "id": 123 or "id": "xxx" from JSON output
    ID_VAL=$(echo "$LAST_OUTPUT" | grep -oP '"id":\s*"?([^",}\s]+)' | head -1 | sed 's/.*:\s*"\{0,1\}//' | sed 's/"\{0,1\}$//')
    echo "  → ID: ${ID_VAL}"
}

echo ""
echo "╔══════════════════════════════════════════════════════════════╗"
echo "║       Bionic-Graph CLI Real Call Test Suite                 ║"
echo "╚══════════════════════════════════════════════════════════════╝"
echo "  Server: ${BASE_URL}"
echo "  Time:   $(date)"
echo ""

# ====================================================================
# 1. HEALTH
# ====================================================================
echo "━━━ 1. HEALTH ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
run "health check" 0 health check

# ====================================================================
# 2. GRAPH
# ====================================================================
echo "━━━ 2. GRAPH ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
run "graph list" 0 graph list
run "graph create" 0 graph create "${TEST_GRAPH}" --description "CLI test graph"
run "graph get-config" 0 graph get-config "${TEST_GRAPH}"
run "graph update-meta" 0 graph update-meta "${TEST_GRAPH}" --description "Updated desc"
run "graph set-config" 0 graph set-config "${TEST_GRAPH}" --config '{"cache_capacity": 8192}'
run "graph set-default" 0 graph set-default "${TEST_GRAPH}"
run "graph set-default (restore)" 0 graph set-default graph0
run "graph delete (force)" 0 graph delete "${TEST_GRAPH}" --force

# ====================================================================
# 3. VERTEX
# ====================================================================
echo "━━━ 3. VERTEX ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
run "vertex create (full params)" 0 vertex create \
    --name "Eddard Stark" \
    --labels '["person","noble","winterfell"]' \
    --keywords '["ned","lord","stark"]' \
    --properties '{"age":45,"house":"Stark","title":"Lord of Winterfell"}'
save_id
VID="$ID_VAL"

run "vertex get-meta" 0 vertex get-meta "${VID}"
run "vertex update" 0 vertex update "${VID}" --name "Eddard Stark (Updated)" --labels '["person","noble"]'
run "vertex update-meta" 0 vertex update-meta "${VID}" --rank 10
run "vertex delete" 0 vertex delete "${VID}" --force

# ====================================================================
# 4. EDGE
# ====================================================================
echo "━━━ 4. EDGE ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
run "vertex create (source)" 0 vertex create --name "Source"
save_id; SVID="$ID_VAL"
run "vertex create (target)" 0 vertex create --name "Target"
save_id; TVID="$ID_VAL"
echo "  → Source=${SVID} Target=${TVID}"

run "edge create (full params)" 0 edge create \
    --source "${SVID}" --target "${TVID}" --name "test_edge" \
    --labels '["relationship","test"]' --keywords '["test","edge"]' \
    --strength 0.85 --properties '{"year":2024,"active":true}'
save_id; EID="$ID_VAL"
echo "  → Edge=${EID}"

run "edge get-meta" 0 edge get-meta "${EID}"
run "edge update" 0 edge update "${EID}" --name "test_edge_updated" --labels '["relationship"]' --strength 0.95
run "edge update-meta" 0 edge update-meta "${EID}" --rank 5
run "edge delete" 0 edge delete "${EID}" --force
run "vertex delete (source)" 0 vertex delete "${SVID}" --force
run "vertex delete (target)" 0 vertex delete "${TVID}" --force

# ====================================================================
# 5. GREMLIN
# ====================================================================
echo "━━━ 5. GREMLIN ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
run "vertex create (gremlin test)" 0 vertex create --name "GremlinNode" --labels '["test"]'
save_id; GVID="$ID_VAL"

run "gremlin execute (V)" 0 gremlin execute --steps "[{\"step\":\"V\",\"ids\":[${GVID}]}]"
run "gremlin execute (has)" 0 gremlin execute --steps '[{"step":"V"},{"step":"has","key":"name","value":"GremlinNode"}]'
run "search" 0 search --text "Gremlin" --mode greedy --limit 10
run "vertex delete (gremlin cleanup)" 0 vertex delete "${GVID}" --force

# ====================================================================
# 6. DOCUMENT
# ====================================================================
echo "━━━ 6. DOCUMENT ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
run "document create" 0 document create --title "Test Document" --content "This is a test document for CLI testing." --tags '["test","cli"]'
save_id; DID="$ID_VAL"
echo "  → Doc ID: ${DID}"

run "document get" 0 document get "${DID}"
run "document update" 0 document update "${DID}" --title "Updated Document" --tags '["test","updated"]'
run "document get-content" 0 document get-content "${DID}"
run "document list" 0 document list
run "document delete" 0 document delete "${DID}"

# ====================================================================
# 7. EXTRACT
# ====================================================================
echo "━━━ 7. EXTRACT ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
run "document create (for extract)" 0 document create --title "Extract Source" --content "Alice works at Acme Corp. Bob is a researcher."
save_id; EXDID="$ID_VAL"
echo "  → Extract Doc ID: ${EXDID}"

# Submit extraction (may succeed or fail depending on LLM config — we just verify it runs)
run "document extract" 0 document extract "${EXDID}"
TASK_ID=$(echo "$LAST_OUTPUT" | grep -oP 'task_id:\s*\K(.+)$' | head -1 | tr -d ' ')
if [ -z "$TASK_ID" ]; then
    TASK_ID=$(echo "$LAST_OUTPUT" | grep -oP '"task_id":\s*"([^"]+)"' | sed 's/.*:"//' | sed 's/"//')
fi
echo "  → Task: ${TASK_ID}"

if [ -n "$TASK_ID" ] && [ "$TASK_ID" != "null" ]; then
    run "task get" 0 task get --task-id "${TASK_ID}"
    run "task wait (timeout)" 1 task wait --task-id "${TASK_ID}" --poll-interval 0.1 --timeout 0.5
fi
run "task list" 0 task list
run "document delete (extract cleanup)" 0 document delete "${EXDID}"

# ====================================================================
# 8. SETTINGS
# ====================================================================
echo "━━━ 8. SETTINGS ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
run "settings get-search" 0 settings get-search
run "settings set-search" 0 settings set-search --config '{"greedy": {"mode": "prefix"}, "exact": {}}'
run "settings get-llm" 0 settings get-llm
run "settings set-llm (default-model)" 0 settings set-llm --default-model "DeepSeek/deepseek-v4-flash"
run "settings get-rank" 0 settings get-rank
run "settings set-rank" 0 settings set-rank --config '{"auto_inc_rank_when_update": true}'
run "settings get-web-search" 0 settings get-web-search
run "settings get-tokenizer" 0 settings get-tokenizer
run "settings add-tokenizer-words" 0 settings add-tokenizer-words --words '["test_cli_word"]'
run "settings remove-tokenizer-words" 0 settings remove-tokenizer-words --words '["test_cli_word"]'

# ====================================================================
# 9. MAAS
# ====================================================================
echo "━━━ 9. MAAS ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
run "proxy openai-models" 0 proxy openai-models
run "proxy openai-chat" 0 proxy openai-chat --messages '[{"role":"user","content":"Hi"}]' --model "DeepSeek/deepseek-v4-flash"

# ====================================================================
# 10. GLOBAL OPTIONS
# ====================================================================
echo "━━━ 10. GLOBAL OPTIONS ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
run "--output json" 0 --output json health check
run "--timeout" 0 --timeout 15 health check
run "--api-key" 0 --api-key "sk-test" health check

# ====================================================================
# 11. ERROR HANDLING
# ====================================================================
echo "━━━ 11. ERROR HANDLING ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
run "graph get-config (not found)" 0 graph get-config "nonexistent_${TS}"
run "graph set-config (invalid JSON)" 2 graph set-config graph0 --config "not-json"

# ====================================================================
# REPORT
# ====================================================================
echo ""
echo "╔══════════════════════════════════════════════════════════════╗"
echo "║                    TEST REPORT                               ║"
echo "╚══════════════════════════════════════════════════════════════╝"
echo "  Total:  $((PASS + FAIL))"
echo "  Passed: ${PASS}"
echo "  Failed: ${FAIL}"
echo ""

if [ "${FAIL}" -gt 0 ]; then
    echo "  Failed tests:"
    echo -e "${FAILED_TESTS}"
    exit 1
else
    echo "  🎉 All tests passed!"
    exit 0
fi
