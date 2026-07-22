"""Prompt templates for the Social Activities CLI pipeline.

All prompts are in English. The LLM is instructed to output JSON matching
the Bionic-Graph vertex/edge SDK signatures:

  entity  -> { name, labels, keywords, properties }
  relation -> { source, target, name, labels, keywords, strength, properties }
"""

# ── Phase 1: Extract from Markdown ────────────────────────────────────

EXTRACT_SYSTEM_PROMPT = """You are a social activity knowledge graph extraction expert. Your task is to parse a detailed Markdown document about a group of friends and their social activities, then extract entities and the relations between them as a JSON object.

The document describes approximately 10 young urban professionals living in Chengdu, their relationships, social activities (dining, cycling, travel, shopping, fitness, work outings, dating, weddings, parenting, tutoring), and their individual plans.

Extraction rules:
1. Extract ALL meaningful entities from the document. Cover every section: characters, relationships, social activities (each event type), locations, and individual plans.
2. Each entity must have a unique, descriptive `name`.
3. Use `labels` for type categorization:
   - People: ["person", "character"]
   - Activities: ["social_activity", "<category>"] where category is one of: dining, cycling, travel, shopping, fitness, work, dating, wedding, parenting, leisure, tutoring, plan
   - Locations: ["location", "<city/district>"]
   - Relationships/statuses: ["relationship", "<type>"]
4. Use `keywords` for searchable terms.
5. Use `properties` to store details (age, occupation, date, venue, bill amount, participants count, etc.). Properties must be flat — only strings, numbers, booleans, or arrays of strings. NO nested objects or dicts inside properties.
6. For relations, use descriptive `name` such as "participates_in", "friend_of", "married_to", "dating", "colleagues", "organizes", "attended", "located_in", "has_plan", etc.
7. The output MUST be valid JSON with exactly two top-level keys: "entities" and "relations".
8. Be thorough — extract at least 60 entities and 80 relations.
"""

EXTRACT_USER_PROMPT_TEMPLATE = """Parse the following social activities Markdown document and extract all entities and relations as a JSON object.

Document content:
```
{document_content}
```

Output format — strictly valid JSON with NO markdown wrapping or extra text:
{{
  "entities": [
    {{
      "name": "Zhang Wei",
      "labels": ["person", "character"],
      "keywords": ["project manager", "chengdu", "married"],
      "properties": {{
        "age": 33,
        "occupation": "Project Manager",
        "residence": "Wuhou District, Chengdu",
        "relationship_status": "married"
      }}
    }},
    {{
      "name": "Monthly Hotpot at Da Miao (June 2026)",
      "labels": ["social_activity", "dining"],
      "keywords": ["hotpot", "chunxi road", "gathering"],
      "properties": {{
        "date": "2026-06-01",
        "venue": "Da Miao Hotpot, Chunxi Road",
        "bill": 886,
        "attendees_count": 10
      }}
    }},
    ... more entities ...
  ],
  "relations": [
    {{
      "source": "Zhang Wei",
      "target": "Monthly Hotpot at Da Miao (June 2026)",
      "name": "participates_in",
      "labels": ["attendance"],
      "strength": 1.0,
      "properties": {{}}
    }},
    {{
      "source": "Zhang Wei",
      "target": "Wang Qiang",
      "name": "friend_of",
      "labels": ["close_friend"],
      "strength": 0.9,
      "properties": {{"years": 12}}
    }},
    ... more relations ...
  ]
}}

Important constraints:
- entity keys: name (required, unique string), labels (list), keywords (list), properties (dict).
- relation keys: source (string = entity name), target (string = entity name), name (required), labels (list), keywords (list), strength (float 0-1), properties (dict).
- Extract ALL characters (at least 10), ALL major activity events (15+), ALL locations (8+).
- Activity entities should use the event name + date as the entity name for uniqueness.
- Extract relationships between people (friend_of, married_to, dating, colleagues).
- Aim for at least 60 entities and 80 relations."""

# ── Phase 2: Plan generation ──────────────────────────────────────────

PLAN_SYSTEM_PROMPT = """You are a social activity planning assistant. Given the current state of a friend group's social graph — their relationships, past activities, individual schedules and interests — generate new social activity plans.

Each plan should describe a social activity that the group (or a subset) could do together. Cover diverse categories:
- Dining and food exploration
- Outdoor activities (cycling, hiking, travel)
- Cultural events (concerts, exhibitions, museums)
- Shopping and leisure
- Fitness and sports
- Celebrations (birthdays, milestones)
- Casual hangouts (board games, mahjong, karaoke)

Output as JSON with "entities" (plan items) and "relations" (connections from characters to plans).

Each plan entity must have:
- name: descriptive and unique
- labels: ["plan", "social_activity", "<category>"]
- properties.category: one of dining, outdoor, cultural, shopping, fitness, celebration, casual
- properties.timeframe: human-readable estimate
- properties.priority: integer 1-10
- properties.status: "pending"
- properties.suggested_organizer: name of the character who would organize it

Connect each plan to the organizing character via a "has_plan" relation, and to participating characters via "invited" relations.
"""

PLAN_USER_PROMPT_TEMPLATE = """Based on the following social graph state, generate new social activity plans for the friend group.

Current graph state:
{graph_summary}

Output JSON format:
{{
  "entities": [
    {{
      "name": "<unique plan name>",
      "labels": ["plan", "social_activity", "<category>"],
      "keywords": [...],
      "properties": {{
        "category": "<dining|outdoor|cultural|shopping|fitness|celebration|casual>",
        "timeframe": "<estimated duration>",
        "priority": <1-10>,
        "status": "pending",
        "suggested_organizer": "<character name>"
      }}
    }},
    ...
  ],
  "relations": [
    {{
      "source": "<organizer character name>",
      "target": "<plan name>",
      "name": "has_plan",
      "labels": ["organization"],
      "strength": 1.0,
      "properties": {{}}
    }},
    {{
      "source": "<participant character name>",
      "target": "<plan name>",
      "name": "invited",
      "labels": ["participation"],
      "strength": 0.8,
      "properties": {{}}
    }},
    ...
  ]
}}

Generate 10-15 diverse social activity plans. Each plan should feel specific to Chengdu and the group's character dynamics."""

# ── Phase 3: Activity simulation ──────────────────────────────────────

EXEC_SYSTEM_PROMPT = """You are a social activity simulation engine. Given a planned social activity and the context of the friend group, simulate the execution of that activity in detail.

Describe what happened during the activity: who participated, what they did, conversations, highlights, funny moments, and outcomes.

Output as JSON with:
- "entities": the activity execution record (name MUST be distinct from the plan name, e.g. "Executed: <plan name>" or "<plan name> - Actual Event")
  - labels: ["activity_execution", "<category>"]
  - properties.execution: detailed 3-5 sentence narrative
  - properties.result: "success" | "partial" | "failed"
  - properties.time_spent_hours: float
  - properties.takeaway: key insight or memorable moment
  - properties.progress_pct: integer 0-100
- "relations": "has_activity" from plan to execution, "participated_in" from characters to execution
- "plan_updates": status/progress changes for the plan vertex
"""

EXEC_USER_PROMPT_TEMPLATE = """Simulate the execution of the following social activity plans for the Chengdu friend group.

Group context:
{person_context}

Plans to execute (sorted by priority):
{plans_json}

Output JSON format:
{{
  "entities": [
    {{
      "name": "Executed: Housewarming Party for Chen Jie & Liu Yang",
      "labels": ["activity_execution", "celebration"],
      "keywords": [...],
      "properties": {{
        "execution": "<3-5 sentence narrative of what happened>",
        "result": "success|partial|failed",
        "time_spent_hours": <float>,
        "takeaway": "<key insight or memorable moment>",
        "progress_pct": <0-100>
      }}
    }},
    ...
  ],
  "relations": [
    {{
      "source": "<plan name>",
      "target": "<execution name>",
      "name": "has_activity",
      "labels": ["execution"],
      "strength": 1.0,
      "properties": {{}}
    }},
    {{
      "source": "<character name>",
      "target": "<execution name>",
      "name": "participated_in",
      "labels": ["attendance"],
      "strength": 0.8,
      "properties": {{}}
    }},
    ...
  ],
  "plan_updates": [
    {{
      "name": "<plan name>",
      "properties": {{
        "status": "in-progress|completed|blocked",
        "progress_pct": <0-100>
      }}
    }}
  ]
}}

Generate one execution per plan. Make the simulation realistic with specific details about Chengdu locations, food, and group dynamics."""


# ── Context builder ───────────────────────────────────────────────────

def build_person_context(markdown_content: str, max_chars: int = 3000) -> str:
    """Extract a concise group summary from the markdown for use in prompts.

    Truncates to max_chars to fit token budget.
    """
    lines = markdown_content.split("\n")
    relevant = []
    capture = False
    for line in lines:
        stripped = line.strip()
        if stripped.startswith("## Characters") or stripped.startswith("## Relationships"):
            capture = True
        elif stripped.startswith("## ") and capture:
            if not any(kw in stripped.lower() for kw in
                ["characters", "relationships", "social activit", "individual"]):
                capture = False
        if capture:
            relevant.append(line)

    text = "\n".join(relevant)
    if len(text) > max_chars:
        text = text[:max_chars] + "\n... [truncated]"
    return text
