# Self-Awareness Knowledge Graph — Example

This example demonstrates how to build and evolve a **self-awareness knowledge graph** using Bionic-Graph and a large language model (LLM). It implements a three-phase lifecycle — **load**, **plan**, and **act** — that mirrors human self-reflection and execution.

## Purpose

Create a living knowledge graph that captures a person's identity, personality, values, skills, interests, social relations, and life plans. The graph serves as an externalized cognitive model that can:

- Store structured self-knowledge extracted from a narrative document
- Generate contextual plans across multiple life dimensions (learning, sports, work, hobbies, social)
- Simulate activity execution and track progress over time
- Provide a foundation for autonomous agents that reflect and act

## Principles

- **Document → Graph**: Self-knowledge starts as a Markdown document; LLM extraction converts it into structured entities and relations.
- **Dedup by Name**: Vertex names are unique identifiers. Re-running load/plan skips existing vertices and only adds new ones.
- **Priority-driven Sorting**: Plans are sorted by `properties.priority` (high > medium > low) then by built-in `rank` as secondary key.
- **Persistent Logging**: Every LLM output is saved as timestamped JSON in `log/` for auditability and replay.
- **Field-Match JSON**: The LLM output schema (`entities` / `relations`) uses the exact same field names as the Bionic-Graph Python SDK (`create_vertex()` / `create_edge()`), allowing direct keyword-argument unpacking.

## Pipeline

```
┌─────────────────────────────────────────────────────────────┐
│  load                                                        │
│  self_soul.md ──► LLM extraction ──► self_soul.json         │
│       │                           │                         │
│       │    ┌──────────────────────┘                         │
│       ▼    ▼                                                 │
│  Bionic-Graph graph (alex-soul)                              │
│  ┌──────────────────────────────────────┐                    │
│  │  Vertex "self" (root)               │                    │
│  │  + 80+ entity vertices             │                    │
│  │  + 80+ relation edges              │                    │
│  └──────────────────────────────────────┘                    │
│                                                              │
│  plan                                                        │
│  Search "my plan interest task activity" → LLM generates plans │
│  → log/plan_<timestamp>.json → loaded into graph              │
│                                                              │
│  act                                                         │
│  Fetch plans sorted by priority → LLM simulates top-N       │
│  → log/activity_<timestamp>.json → loaded into graph         │
│  → plan status updated                                       │
└─────────────────────────────────────────────────────────────┘
```

### Phase 1: load

1. Read `self_soul.md` (a detailed self-description in Markdown)
2. Call LLM to extract entities and relations as JSON
3. The root vertex **must** be named `"self"`
4. Save the extracted JSON to `self_soul.json`
5. Load into the Bionic-Graph graph (dedup by vertex name)

### Phase 2: plan

1. Search the graph for `"my plan interest task activity"` to gather context
2. Call LLM to generate plans across 5 dimensions:
   - **Learning** — technical skills, languages, academic
   - **Sports** — running, yoga, fitness goals
   - **Work** — research, coding, projects, deadlines
   - **Hobbies** — photography, cooking, creative outlets
   - **Social** — friends, family, community
3. Each plan gets labels `["plan", "task", "<dimension>", "<priority-tag>"]`
4. Plans are sorted by `priority` property, then by `rank`

### Phase 3: act

1. Fetch all plan vertices sorted by `properties.priority` descending
2. Select the top N plans (default 3)
3. Call LLM to simulate each activity execution
4. Create `Activity` vertices connected via `has_activity` edges
5. Update each plan's `status` and `progress_pct`

## Directory Structure

```
examples/self_awareness/
├── README.md              # This file
├── self_soul.md           # Self-description Markdown document
├── cli.py                 # CLI entry — load / plan / act
├── llm.py                 # LLM call wrapper (MaaS proxy)
├── prompts.py             # Prompt templates
├── graph_utils.py         # Graph utility functions
├── self_soul.json         # [generated] Extracted KG from load phase
├── log/                   # [generated] Timestamped output files
│   ├── plan_<timestamp>.json
│   └── activity_<timestamp>.json
└── .gitignore             # log/ is gitignored
```

## CLI Usage

```
Usage:
  python cli.py <command> [OPTIONS]

Commands:
  load   Load self-awareness from a Markdown document into the graph
           Options:
             --md PATH             Markdown document path (default: self_soul.md)
             --graph TEXT           Graph name (default: self-awareness)
             --model TEXT           LLM model name (default: settings default_model)
             --output PATH          Output JSON file path (default: self_soul.json)
             --base-url TEXT        Backend URL (default: http://127.0.0.1:8080)
             --force                Force re-extract and overwrite existing vertices

  plan   Reflect on graph state and generate next-phase plans
           Options:
             --graph TEXT           Graph name (default: self-awareness)
             --model TEXT           LLM model name (default: settings default_model)
             --output PATH          Output file path (default: log/plan_<timestamp>.json)
             --base-url TEXT        Backend URL (default: http://127.0.0.1:8080)

  act    Execute top-N activities sorted by priority
           Options:
             --count N             Number of activities to simulate (default: 3)
             --graph TEXT           Graph name (default: self-awareness)
             --model TEXT           LLM model name (default: settings default_model)
             --output PATH          Output file path (default: log/activity_<timestamp>.json)
             --base-url TEXT        Backend URL (default: http://127.0.0.1:8080)

Global options:
  --help           Show this help message
```

### Quick Start

```bash
# Prerequisites: Bionic-Graph backend running on http://127.0.0.1:8080

# Phase 1: Load self-awareness from the Markdown document
python cli.py load --graph my-soul

# Phase 2: Reflect and generate plans
python cli.py plan --graph my-soul

# Phase 3: Simulate top-3 activities
python cli.py act --graph my-soul --count 3
```

## self_soul.md — Required Content

The Markdown document (`self_soul.md`) is the single source of truth for the knowledge graph. It should describe the person in detail across the following sections. Each section is parsed by the LLM and converted to entities and relations.

### Required Sections

| Section | Description | Example entities |
|---------|-------------|-----------------|
| `## Identity information` | Name, age, nationality, education, occupation, residence | `self`, `Vancouver`, `UBC` |
| `## Physical information` | Height, weight, health, allergies, injuries | — (stored as properties of `self`) |
| `## Mental information` | Intelligence, memory, emotion, stress, dreams | — (stored as properties) |
| `## Personality` | MBTI, Big Five, core traits, quirks | `INTP`, `Big Five Profile` |
| `## Value orientation` | Core values ranked by importance | `Truth`, `Growth`, `Kindness` |
| `## Motivations` | Intrinsic/extrinsic motivations, fears | `Understanding Minds`, `Mastery` |
| `## Interests` | Intellectual interests and hobbies | `Cognitive Science`, `Running`, `Chess` |
| `## Skills` | Technical, research, language, soft skills | `Rust`, `Python`, `Technical Writing` |
| `## Tasks` | Active tasks with priority and deadlines | `GraphRAG Paper Revision` |
| `## Plans` | Short/mid/long-term plans and life goals | `One-Year Plan`, `Life Goals` |
| `## Stories` | Personal narratives and milestones | `Story: The Open-Source Epiphany` |
| `## Social relations` | Family, friends, colleagues, community | `Maya Patel`, `Dr. Anika Sharma` |
| `## Social activities` | Weekly schedule, annual events, contributions | `Rust Meetup`, `Running Club` |

### Extraction Coverage

The LLM is instructed to extract at least **40 entities and 50 relations** to capture the richness of the document. Each entity must have a unique `name`. The root entity must be named `"self"`.

### Example

A complete example is provided in the bundled `self_soul.md`, which describes a fictional cognitive science researcher named **Alex Chen** (age 28, living in Vancouver). You can replace it with your own self-description following the same section structure.

## Data Flow Summary

```
                LLM (MaaS proxy via Bionic-Graph backend)
                ▲         │
                │         ▼
self_soul.md ──►  extraction  ──► self_soul.json ──► Graph
                                              │
                     Graph state ──► LLM ──► log/plan_<timestamp>.json ──► Graph
                                              │
                     Plans ──► LLM ──► log/activity_<timestamp>.json ──► Graph
                                                            │
                                              Plan statuses updated
```

## Graph Schema

```
Vertex "self" (labels: ["person", "self"])
  │
  ├── has_plan ──► Plan vertex (labels: ["plan", "task", "<dimension>", "<priority>"])
  │                  │
  │                  └── has_activity ──► Activity vertex (labels: ["activity", "<dimension>"])
  │
  ├── has_skill ──► Skill vertex
  ├── interested_in ──► Interest vertex
  ├── values ──► Value vertex
  ├── has_friend ──► Person vertex
  ├── has_colleague ──► Person vertex
  ├── working_on ──► Task vertex
  ├── resides_in ──► Location vertex
  └── ... (30+ relation types)
```
