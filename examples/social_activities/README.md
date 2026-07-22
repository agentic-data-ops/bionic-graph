# Social Activities Knowledge Graph — Example

This example demonstrates how to build and evolve a **social activities knowledge graph** using Bionic-Graph and a large language model (LLM). It implements a three-phase lifecycle — **load**, **plan**, and **act** — that models the social life of a friend group.

## Purpose

Create a living knowledge graph that captures a group of friends, their relationships, social activities, individual plans, and life events. The graph serves as an externalized social model that can:

- Store structured information about a friend group (10 characters) extracted from a narrative document
- Generate new social activity plans across dining, outdoor, cultural, shopping, fitness, celebration, and casual categories
- Simulate activity execution with detailed narratives and track outcomes
- Provide a foundation for simulating group dynamics and social behavior

## Principles

- **Document → Graph**: Social knowledge starts as a Markdown document; LLM extraction converts it into structured entities and relations.
- **Dedup by Name**: Vertex names are unique identifiers. Re-running load/plan skips existing vertices and only adds new ones.
- **Priority-driven Sorting**: Plans are sorted by `properties.priority` descending, then by built-in `rank` as secondary key.
- **Persistent Logging**: Every LLM output is saved as timestamped JSON in `log/` for auditability and replay.
- **Field-Match JSON**: The LLM output schema (`entities` / `relations`) uses the exact same field names as the Bionic-Graph Python SDK (`create_vertex()` / `create_edge()`).
- **Multi-character**: Unlike the self-awareness example (single `"self"` root), this graph has 10+ character vertices as independent entities.

## Pipeline

```
┌─────────────────────────────────────────────────────────────┐
│  load                                                        │
│  social_activities.md ──► LLM extraction ──► log/social...   │
│       │                           │                          │
│       │    ┌──────────────────────┘                          │
│       ▼    ▼                                                  │
│  Bionic-Graph graph (social-graph)                            │
│  ┌──────────────────────────────────────┐                     │
│  │  10 characters + 15+ activities     │                     │
│  │  8+ locations + 80+ relation edges  │                     │
│  └──────────────────────────────────────┘                     │
│                                                               │
│  plan                                                         │
│  Search "activity plan" → LLM generates plans (7 categories)  │
│  → log/plan_activities_<timestamp>.json → loaded into graph   │
│                                                               │
│  act                                                          │
│  Fetch plans sorted by priority → LLM simulates top-N        │
│  → log/exec_activities_<timestamp>.json → loaded into graph  │
│  → plan status updated                                        │
└──────────────────────────────────────────────────────────────┘
```

### Phase 1: load

1. Read `social_activities.md` (a detailed group description in Markdown)
2. Call LLM to extract entities and relations as JSON
3. Save the extracted JSON to `log/social_activities.json`
4. Load into the Bionic-Graph graph (dedup by vertex name)

### Phase 2: plan

1. Search the graph for `"activity plan"` to gather context (falls back to full scan if search returns nothing)
2. Sort results by `priority` + `rank`
3. Call LLM to generate new social activity plans across 7 categories:
   - **Dining** — hotpot gatherings, restaurant exploration, dinner parties
   - **Outdoor** — cycling, hiking, travel, day trips
   - **Cultural** — museums, opera, exhibitions
   - **Shopping** — mall trips, boutique visits, styling days
   - **Fitness** — yoga, bootcamps, running, workouts
   - **Celebration** — birthdays, housewarmings, baby showers
   - **Casual** — board games, mahjong, karaoke
4. Each plan has labels `["plan", "social_activity", "<category>"]`
5. Plans are connected to characters via `has_plan` (organizer) and `invited` (participants) relations

### Phase 3: act

1. Fetch all plan vertices sorted by `properties.priority` descending
2. Select the top N plans (default 3)
3. Call LLM to simulate each activity execution with character participation details
4. Create `ActivityExecution` vertices connected via `has_activity` edges
5. Create `participated_in` edges connecting characters to activity executions
6. Update each plan's `status` and `progress_pct`

## Directory Structure

```
examples/social_activities/
├── README.md                  # This file
├── social_activities.md       # Group social activity Markdown document
├── cli.py                     # CLI entry — load / plan / act
├── llm.py                     # LLM call wrapper (MaaS proxy)
├── prompts.py                 # Prompt templates
├── graph_utils.py             # Graph utility functions
├── log/                       # [generated] Timestamped output files
│   ├── social_activities.json
│   ├── plan_activities_<timestamp>.json
│   └── exec_activities_<timestamp>.json
└── .gitignore                 # log/ is gitignored
```

## CLI Usage

```
Usage:
  python cli.py <command> [OPTIONS]

Commands:
  load   Load social activities from a Markdown document into the graph
           Options:
             --md PATH             Markdown document path (default: social_activities.md)
             --graph TEXT           Graph name (default: social-graph)
             --model TEXT           LLM model name (default: settings default_model)
             --output PATH          Output JSON file path (default: log/social_activities.json)
             --base-url TEXT        Backend URL (default: http://127.0.0.1:8080)
             --force                Delete and recreate graph before loading

  plan   Generate new social activity plans
           Options:
             --graph TEXT           Graph name (default: social-graph)
             --model TEXT           LLM model name (default: settings default_model)
             --output PATH          Output file path (default: log/plan_activities_<timestamp>.json)
             --base-url TEXT        Backend URL (default: http://127.0.0.1:8080)

  act    Simulate social activity execution
           Options:
             --count N             Number of activities to simulate (default: 3)
             --graph TEXT           Graph name (default: social-graph)
             --model TEXT           LLM model name (default: settings default_model)
             --output PATH          Output file path (default: log/exec_activities_<timestamp>.json)
             --base-url TEXT        Backend URL (default: http://127.0.0.1:8080)

Global options:
  --help           Show this help message
```

### Quick Start

```bash
# Prerequisites: Bionic-Graph backend running on http://127.0.0.1:8080

# Phase 1: Load social activities from the Markdown document
python cli.py load --graph my-social-group

# Phase 2: Generate new social activity plans
python cli.py plan --graph my-social-group

# Phase 3: Simulate top-3 activities
python cli.py act --graph my-social-group --count 3
```

## social_activities.md — Required Content

The Markdown document (`social_activities.md`) should describe a group of people and their social interactions. It is parsed by the LLM and converted to entities and relations across the following sections.

### Required Sections

| Section | Description | Example entities |
|---------|-------------|-----------------|
| `## Characters` | Individual profiles (name, age, occupation, personality, relationship status) | `Zhang Wei`, `Wang Qiang`, `Chen Jie` |
| `## Relationships` | How characters know each other (friendship, family, colleagues, romantic) | `friend_of`, `married_to`, `colleagues` |
| `## Social Activities` | Past events with details (date, venue, participants, bill, highlights) | `Monthly Hotpot at Da Miao (June 2026)` |
| `## Individual Plans` | Each character's next-phase plans across life dimensions | `Zhang Wei's Plans`, `Li Na's Plans` |
| `## Group Calendar` | Upcoming events organized by week | `Baby Shower (August Week 1)` |

### Activity Types Covered

| Activity Type | Examples |
|---------------|----------|
| **Dining** | Hotpot gatherings, restaurant exploration, dinner parties, brunch clubs, late-night snacks |
| **Travel** | Spring Festival trips, long weekends, museum day trips |
| **Cycling & Fitness** | Greenway rides, marathon training, gym sessions, yoga classes |
| **Work** | Business trips, startup events, conferences, late-night coding |
| **Shopping** | Mall trips, boutique visits, styling days |
| **Dating** | Blind dates, relationship milestones, breakups |
| **Celebration** | Birthdays, weddings, baby showers, housewarmings |
| **Parenting** | Daycare, tutoring classes, children's activities |
| **Casual** | Board games, mahjong, karaoke, movie outings |

### Extraction Coverage

The LLM is instructed to extract at least **60 entities and 80 relations** to capture the richness of the document. Each character becomes a vertex; each distinct event becomes a vertex. Relations include character-to-character connections and character-to-event participation.

### Example

A complete example is provided in the bundled `social_activities.md`, which describes a group of 10 young urban professionals living in **Chengdu**: Zhang Wei (project manager), Wang Qiang (sales director), Chen Jie (fitness coach), Liu Yang (kindergarten teacher), Zhao Lei (startup founder), Sun Fang (teacher), Zhou Ming (accountant), Lin Xiao (boutique owner), Huang Yu (software engineer), and their families. You can replace it with your own group description.

## Data Flow Summary

```
                LLM (MaaS proxy via Bionic-Graph backend)
                ▲         │
                │         ▼
social_activities.md ──►  extraction  ──► log/social_activities.json ──► Graph
                                                    │
                         Graph state ──► LLM ──► log/plan_activities_<ts>.json ──► Graph
                                                    │
                         Plans ──► LLM ──► log/exec_activities_<ts>.json ──► Graph
                                                                              │
                                                                Plan statuses updated
```

## Graph Schema

```
Character vertices (labels: ["person", "character"])
  │
  ├── friend_of ──► Character vertex
  ├── married_to ──► Character vertex
  ├── dating ──► Character vertex
  ├── colleague ──► Character vertex
  ├── participates_in ──► SocialActivity vertex
  │                           │
  │                           ├── labels: ["social_activity", "<category>"]
  │                           └── properties: date, venue, bill, participants_count
  │
  ├── has_plan ──► Plan vertex (labels: ["plan", "social_activity", "<category>"])
  │                  │
  │                  └── has_activity ──► ActivityExecution vertex
  │                                        │
  │                                        ├── labels: ["activity_execution", "<category>"]
  │                                        └── properties: execution, result, takeaway, progress_pct
  │
  ├── invited ──► Plan vertex
  ├── lives_in ──► Location vertex
  ├── works_at ──► Organization vertex
  └── ... (10+ relation types)
```

## Differences from self_awareness Example

| Aspect | self_awareness | social_activities |
|--------|---------------|-------------------|
| **Root entity** | Single `"self"` vertex | 10+ character vertices, no single root |
| **Relations** | Star pattern (self → everything) | Mesh pattern (characters ↔ characters, characters ↔ events) |
| **Plan categories** | 5 life dimensions | 7 social activity categories |
| **Act output** | Single `has_activity` edge per plan | `has_activity` + multiple `participated_in` edges |
| **Search query** | `"my plan interest task activity"` | `"activity plan"` |
| **Document size** | ~29KB self-description | ~26KB group description |
