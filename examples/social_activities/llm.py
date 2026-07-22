"""LLM call wrapper using Bionic-Graph MaaS proxy."""

from __future__ import annotations

import json
import time
from typing import Optional

from bionic_graph import Client


def call_llm(
    system_prompt: str,
    user_prompt: str,
    model: Optional[str] = None,
    client: Optional[Client] = None,
    max_retries: int = 2,
    timeout: float = 120.0,
) -> str:
    """Call the LLM via Bionic-Graph MaaS proxy and return the response text.

    Args:
        system_prompt: System-level instruction.
        user_prompt: User message content.
        model: Optional model override (uses settings default_model if None).
        client: Reusable Client instance. Creates a new one if None.
        max_retries: Number of retries on failure (default 2).
        timeout: Request timeout in seconds (default 120).

    Returns:
        The LLM response content as a plain string.
    """
    close_client = False
    if client is None:
        client = Client(timeout=timeout)
        close_client = True

    messages = [
        {"role": "system", "content": system_prompt},
        {"role": "user", "content": user_prompt},
    ]

    last_error = None
    for attempt in range(1 + max_retries):
        try:
            resp = client.chat_completion(messages=messages, model=model, stream=False)
            content = resp.get("choices", [{}])[0].get("message", {}).get("content", "")
            if content.strip():
                return content.strip()
            last_error = "Empty response from LLM"
        except Exception as e:
            last_error = str(e)
            err_str = str(e)
            if hasattr(e, "body") and e.body:
                err_str += f" | body: {e.body[:500]}"
            print(f"  ⚠️  LLM call attempt {attempt + 1} failed: {err_str}")
            if attempt < max_retries:
                time.sleep(2.0 * (attempt + 1))
    else:
        if close_client:
            client.close()
        raise RuntimeError(f"LLM call failed after {max_retries} retries: {last_error}")


def call_llm_json(
    system_prompt: str,
    user_prompt: str,
    model: Optional[str] = None,
    client: Optional[Client] = None,
    max_retries: int = 2,
    timeout: float = 120.0,
) -> dict:
    """Call the LLM and parse the response as JSON.

    Same parameters as call_llm(). Returns a parsed dict.
    The system prompt should instruct the LLM to output valid JSON.
    This function extracts the first ```json ... ``` block if present,
    or tries to parse the entire response as JSON.
    """
    text = call_llm(system_prompt, user_prompt, model, client, max_retries, timeout)

    # Try extracting ```json ... ``` block first
    if "```json" in text:
        start = text.index("```json") + 7
        end = text.index("```", start) if "```" in text[start:] else len(text)
        text = text[start:end].strip()
    elif "```" in text:
        start = text.index("```") + 3
        end = text.index("```", start) if "```" in text[start:] else len(text)
        text = text[start:end].strip()

    return json.loads(text)
