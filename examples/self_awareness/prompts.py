"""Prompt templates used by the Self-Awareness CLI pipeline.

All prompts are in English. The LLM is instructed to output JSON matching
the Bionic-Graph vertex/edge SDK signatures:

  entity  -> { name, labels, keywords, properties }
  relation -> { source, target, name, labels, keywords, strength, properties }
"""

# ── Phase 1: Extract from Markdown ────────────────────────────────────

EXTRACT_SYSTEM_PROMPT = """You are a knowledge graph extraction expert. Your task is to parse a detailed self-awareness Markdown document and extract entities and the relations between them as a JSON object.

The root entity MUST be named "self" — it represents the person described in the document.

Extraction rules:
1. Extract ALL meaningful entities from the document. Cover every section: identity, physical, mental, personality, values, motivations, interests, skills, tasks, stories, social relations, social activities.
2. Each entity must have a unique, descriptive `name`.
3. Use `labels` for type categorization (e.g., "person", "skill", "interest", "location", "task", "value", "trait", "story", "social_relation", "activity").
4. Use `keywords` for searchable terms.
5. Use `properties` to store quantitative/qualitative details (age, scores, descriptions, statuses).
6. For relations, use `source` and `target` as the entity `name` (string), not IDs. Use descriptive `name` for the relation type (e.g., "has_skill", "resides_in", "values", "interested_in", "working_on").
7. Be thorough — extract at least 40+ entities and 50+ relations to capture the richness of the document.
8. The output MUST be valid JSON with exactly two top-level keys: "entities" and "relations".
"""

EXTRACT_USER_PROMPT_TEMPLATE = """Parse the following self-awareness Markdown document and extract all entities and relations as a JSON object.

Document content:
```
{document_content}
```

Output format — strictly valid JSON with NO markdown wrapping or extra text:
{{
  "entities": [
    {{
      "name": "self",
      "labels": ["person", "self"],
      "keywords": [...],
      "properties": {{ ... }}
    }},
    ... more entities ...
  ],
  "relations": [
    {{
      "source": "self",
      "target": "<entity_name>",
      "name": "<relation_type>",
      "labels": [...],
      "strength": 1.0,
      "properties": {{}}
    }},
    ... more relations ...
  ]
}}

Important constraints:
- The root entity MUST be named "self".
- entity keys: name (required, unique string), labels (list of strings), keywords (list of strings), properties (dict).
- relation keys: source (string = entity name), target (string = entity name), name (required string), labels (list), keywords (list), strength (float 0-1), properties (dict).
- Extract entities from ALL sections: identity, physical, mental, personality, values, motivations, interests, skills, tasks, stories, social relations, social activities.
- Aim for at least 40 entities and 50 relations."""

# ── Phase 2: Plan generation ──────────────────────────────────────────

PLAN_SYSTEM_PROMPT = """You are a self-reflection and planning assistant. Given the current state of a person's life extracted from their knowledge graph (interests, tasks, skills, social relations, etc.), generate a comprehensive next-phase plan.

The plan should cover these 5 dimensions:
1. **Learning** — technical skills, languages, academic pursuits
2. **Sports** — fitness, running, training goals
3. **Work** — research, coding, projects, deadlines
4. **Hobbies** — photography, cooking, gaming, creative outlets
5. **Social** — friends, family, community involvement

Output the plan as a JSON object with "entities" (the plan items) and "relations" (connections from "self" to each plan, and optional dependencies between plans).

Each plan entity must have:
- labels containing "plan", "task" and the dimension (e.g., ["plan", "task", "work", "high-priority"])
- properties.dimension = one of "learning", "sports", "work", "hobbies", "social"
- properties.timeframe = human-readable time estimate
- properties.priority = integer 1-10
- properties.status = "pending"
"""

PLAN_USER_PROMPT_TEMPLATE = """Based on the following information extracted from my knowledge graph, generate a next-phase plan covering learning, sports, work, hobbies, and social dimensions.

Current graph state:
{graph_summary}

Output JSON format:
{{
  "entities": [
    {{
      "name": "<unique plan name>",
      "labels": ["plan", "task", "<dimension>", "<priority-tag>"],
      "keywords": [...],
      "properties": {{
        "dimension": "<learning|sports|work|hobbies|social>",
        "timeframe": "<estimated duration>",
        "priority": <1-10>,
        "status": "pending"
      }}
    }},
    ...
  ],
  "relations": [
    {{
      "source": "self",
      "target": "<plan_name>",
      "name": "has_plan",
      "labels": ["ownership"],
      "strength": 1.0,
      "properties": {{}}
    }}
  ]
}}

Generate 2-3 plan items per dimension (10-15 total). Each plan must be specific and actionable."""

# ── Phase 3: Activity simulation ──────────────────────────────────────

ACT_SYSTEM_PROMPT = """You are a life simulation engine. Given a person's plan, simulate the execution of that activity in detail as if it really happened.

Describe what was done, how it went, what was learned, and what the outcome was.

Output as JSON with "entities" (the activity record), "relations" (connection from plan to activity), and "plan_updates" (status/progress changes to the plan vertex).

The activity entity uses:
- labels containing "activity" and the dimension
- properties.execution = detailed narrative of what happened (2-4 sentences)
- properties.result = "success" | "partial" | "failed"
- properties.time_spent_hours = float
- properties.takeaway = key insight learned
- properties.progress_pct = integer 0-100 (how much of the plan is complete after this activity)

plan_updates is a list of objects with "name" (matching the plan name) and "properties" (fields to update, e.g. status, progress_pct).
"""

ACT_USER_PROMPT_TEMPLATE = """Simulate the execution of the following plan items for the person described below.

Person context:
{person_context}

Plans to execute (sorted by priority/rank):
{plans_json}

Output JSON format:
{{
  "entities": [
    {{
      "name": "<activity name>",
      "labels": ["activity", "<dimension>"],
      "keywords": [...],
      "properties": {{
        "execution": "<detailed 2-4 sentence narrative>",
        "result": "success|partial|failed",
        "time_spent_hours": <float>,
        "takeaway": "<key insight>",
        "progress_pct": <0-100>
      }}
    }},
    ...
  ],
  "relations": [
    {{
      "source": "<plan_name>",
      "target": "<activity_name>",
      "name": "has_activity",
      "labels": ["execution"],
      "strength": 1.0,
      "properties": {{}}
    }},
    ...
  ],
  "plan_updates": [
    {{
      "name": "<plan_name>",
      "properties": {{
        "status": "in-progress|completed|blocked",
        "progress_pct": <0-100>
      }}
    }},
    ...
  ]
}}

Generate one activity entity per plan item. Make the simulation realistic and detailed."""

# ── Person context builder ────────────────────────────────────────────

def build_person_context(markdown_content: str, max_chars: int = 3000) -> str:
    """Extract a concise person summary from the markdown for use in prompts.

    Truncates to max_chars to fit token budget.
    """
    lines = markdown_content.split("\n")
    # Extract identity section + first 2-3 lines of other key sections
    relevant = []
    capture = False
    for line in lines:
        stripped = line.strip()
        if stripped.startswith("## ") and any(kw in stripped.lower() for kw in
            ["identify", "identity", "personality", "value", "motivation", "interests", "skills"]):
            capture = True
        elif stripped.startswith("## ") and "information" in stripped.lower():
            capture = True
        elif stripped.startswith("## ") and capture:
            # Stop at next major section that's not in our list
            if not any(kw in stripped.lower() for kw in
                ["mental", "physical", "story", "social", "task", "plan"]):
                capture = False
        if capture:
            relevant.append(line)

    text = "\n".join(relevant)
    if len(text) > max_chars:
        text = text[:max_chars] + "\n... [truncated]"
    return text
