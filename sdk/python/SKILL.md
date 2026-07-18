# Bionic-Graph CLI — `bgcli`

A Python command-line tool to interact with a Bionic-Graph knowledge graph server via REST API.

## Installation

```bash
# Install directly from GitHub
pip install git+https://github.com/agentic-data-ops/bionic-graph.git#subdirectory=sdk/python

# Or install from source after cloning
cd sdk/python
pip install .
```

## Quick Start

```bash
# Set backend URL (default: http://127.0.0.1:8080)
export BIONIC_GRAPH_BASE_URL=http://127.0.0.1:8080

# Check server health
bgcli health check

# List all graphs
bgcli graph list

# Create vertices (characters from Game of Thrones)
bgcli vertex create --name "Eddard Stark" --labels '["person","stark"]' --properties '{"title":"Lord of Winterfell"}'
bgcli vertex create --name "Catelyn Stark" --labels '["person","stark","tully"]'
bgcli vertex create --name "Jon Snow" --labels '["person","stark","bastard"]'

# Create relationships (edges)
bgcli edge create --source 1 --target 2 --name "married_to" --strength 0.9
bgcli edge create --source 1 --target 3 --name "father_of" --strength 0.8

# Search the graph
bgcli gremlin search --text "Stark"

# Execute a Gremlin query
bgcli gremlin execute --steps '[{"step":"V","ids":[1]},{"step":"expand"}]'
```

## Command Structure

```
bgcli [global options] <topic> <action> [arguments]
```

### Global Options

| Option | Env Variable | Default | Description |
|--------|-------------|---------|-------------|
| `--base-url` | `BIONIC_GRAPH_BASE_URL` | `http://127.0.0.1:8080` | Backend server URL |
| `--api-key` | `BIONIC_GRAPH_API_KEY` | — | API key |
| `--timeout` | — | `30.0` | Request timeout (seconds) |
| `--output` | — | `text` | Output format: `text` or `json` |

### Topics and Actions

| Topic | Actions | Description |
|-------|---------|-------------|
| `health` | `check` | Check server health |
| `graph` | `list`, `create`, `set-default`, `delete`, `update-meta`, `get-config`, `set-config` | Graph lifecycle |
| `vertex` | `create`, `update`, `delete`, `get-meta`, `update-meta` | Vertex CRUD |
| `edge` | `create`, `update`, `delete`, `get-meta`, `update-meta` | Edge CRUD |
| `gremlin` | `execute`, `search` | Gremlin queries & search |
| `document` | `list`, `create`, `get`, `update`, `delete`, `get-content` | Document management |
| `extract` | `submit`, `get-task`, `list-tasks`, `wait` | Knowledge extraction |
| `settings` | `get-search`, `set-search`, `get-llm`, `set-llm`, `get-rank`, `set-rank`, `get-web-search`, `set-web-search`, `proxy`, `get-tokenizer`, `add-tokenizer-words`, `remove-tokenizer-words` | All settings |
| `maas` | `list-models`, `chat` | MaaS proxy |
| **`chat`** | — | **Interactive chat session** |

## Interactive Chat

```bash
# Start chat (web search and graph search enabled by default)
bgcli chat

# With custom options
bgcli chat --model "DeepSeek/deepseek-v4-flash" \
           --web-search --graph-search \
           --extract-keywords --graph graph0 \
           --search-mode greedy

# Disable web search
bgcli chat --no-web-search
```

Chat session internal commands:

| Command | Description |
|---------|-------------|
| `/exit` or `/quit` | Exit chat |
| `/clear` | Clear conversation history |
| `/graph <name>` | Switch active graph |
| `/help` | Show help |

### Chat Workflow

```
User input → LLM extracts keywords (optional) → Web search (optional)
→ Graph search (optional) → Merge context → LLM responds
```

## Output Format

Default output is human-readable text. Use `--output json` for raw JSON:

```bash
bgcli --output json health check
bgcli --output json vertex get-meta 1
```

## Complete Example: A Song of Ice and Fire

```bash
# 1. Create a dedicated graph
bgcli graph create got --description "A Song of Ice and Fire characters and relationships"

# 2. Add major characters (vertices)
bgcli vertex create --name "Eddard Stark" --labels '["person","stark"]' --properties '{"title":"Lord of Winterfell","status":"deceased"}' --graph got
bgcli vertex create --name "Catelyn Stark" --labels '["person","stark","tully"]' --properties '{"title":"Lady of Winterfell"}' --graph got
bgcli vertex create --name "Robb Stark" --labels '["person","stark"]' --properties '{"title":"King in the North"}' --graph got
bgcli vertex create --name "Sansa Stark" --labels '["person","stark"]' --properties '{"title":"Lady of Winterfell"}' --graph got
bgcli vertex create --name "Arya Stark" --labels '["person","stark","faceless man"]' --graph got
bgcli vertex create --name "Bran Stark" --labels '["person","stark","greenseer","three-eyed raven"]' --graph got
bgcli vertex create --name "Jon Snow" --labels '["person","stark","targaryen","lord commander"]' --properties '{"title":"King in the North"}' --graph got
bgcli vertex create --name "Daenerys Targaryen" --labels '["person","targaryen"]' --properties '{"title":"Mother of Dragons","status":"deceased"}' --graph got
bgcli vertex create --name "Tyrion Lannister" --labels '["person","lannister"]' --properties '{"title":"Hand of the King"}' --graph got
bgcli vertex create --name "Jaime Lannister" --labels '["person","lannister","kingsguard"]' --graph got
bgcli vertex create --name "Cersei Lannister" --labels '["person","lannister"]' --properties '{"title":"Queen of the Seven Kingdoms"}' --graph got
bgcli vertex create --name "Winterfell" --labels '["location","castle"]'
bgcli vertex create --name "King's Landing" --labels '["location","city"]'

# 3. Create family relationships (edges)
# Stark family
bgcli edge create --source 1 --target 2 --name "married_to" --strength 0.9 --graph got       # Ned ←→ Catelyn
bgcli edge create --source 1 --target 3 --name "father_of" --strength 0.8 --graph got       # Ned → Robb
bgcli edge create --source 1 --target 4 --name "father_of" --strength 0.8 --graph got       # Ned → Sansa
bgcli edge create --source 1 --target 5 --name "father_of" --strength 0.8 --graph got       # Ned → Arya
bgcli edge create --source 1 --target 6 --name "father_of" --strength 0.8 --graph got       # Ned → Bran
bgcli edge create --source 1 --target 7 --name "uncle_of" --strength 0.6 --graph got         # Ned → Jon (uncle, later revealed as aunt's son)

# Lannister family
bgcli edge create --source 9 --target 10 --name "brother_of" --strength 0.7 --graph got      # Tyrion → Jaime
bgcli edge create --source 9 --target 11 --name "brother_of" --strength 0.3 --graph got      # Tyrion → Cersei (estranged)
bgcli edge create --source 10 --target 11 --name "lovers" --strength 0.95 --graph got        # Jaime ←→ Cersei (secret)

# Locations
bgcli edge create --source 1 --target 12 --name "rules" --strength 0.7 --graph got          # Ned → Winterfell
bgcli edge create --source 11 --target 13 --name "rules" --strength 0.6 --graph got         # Cersei → King's Landing

# Plot relationships
bgcli edge create --source 7 --target 8 --name "allied_with" --strength 0.5 --graph got     # Jon ←→ Daenerys
bgcli edge create --source 9 --target 8 --name "served" --strength 0.6 --graph got          # Tyrion → Daenerys

# 4. Search for all Stark family
bgcli gremlin search --text "Stark" --graph got

# 5. Expand a character's relationships
bgcli gremlin execute --steps '[{"step":"V","ids":[1]},{"step":"expand"}]' --graph got
```

## Python SDK Programming Interface

```python
from bionic_graph import Client

client = Client(base_url="http://127.0.0.1:8080")

# Health check
print(client.health().status)

# Create characters from A Song of Ice and Fire
ned = client.create_vertex(
    "Eddard Stark",
    labels=["person", "stark"],
    properties={"title": "Lord of Winterfell"},
)
print(f"Vertex ID: {ned.id}")

jon = client.create_vertex(
    "Jon Snow",
    labels=["person", "stark", "bastard"],
    properties={"title": "Lord Commander"},
)

# Create relationship
client.create_edge(ned.id, jon.id, name="guardian_of", strength=0.7)

# Run a Gremlin query to explore
result = client.execute_gremlin([
    {"step": "V", "ids": [ned.id]},
    {"step": "expand"},
])
for item in result.data:
    print(f"  {item}")
```
