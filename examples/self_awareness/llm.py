"""LLM call wrapper using Bionic-Graph MaaS proxy."""

from __future__ import annotations

import json
import os
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
            # Extract server response body if available
            err_str = str(e)
            if hasattr(e, 'body') and e.body:
                err_str += f" | body: {e.body[:500]}"
            print(f"  ⚠️  LLM call attempt {attempt + 1} failed: {err_str}")
            if attempt < max_retries:
                time.sleep(2.0 * (attempt + 1))
    else:
        if close_client:
            client.close()
        raise RuntimeError(f"LLM call failed after {max_retries} retries: {last_error}")


def call_llm_stream_to_file(
    system_prompt: str,
    user_prompt: str,
    tmp_path: str,
    model: Optional[str] = None,
    client: Optional[Client] = None,
    timeout: float = 300.0,
) -> dict:
    """Stream the LLM response to a temp file, then validate it as JSON.

    - stream=True: content chunks are appended to tmp_path as they arrive.
    - After the stream completes, the temp file is read back and parsed as JSON.
    - No retries: any LLM/transport/JSON error raises immediately.

    Args:
        system_prompt: System-level instruction.
        user_prompt: User message content.
        tmp_path: Temp file path to stream content into.
        model: Optional model override (uses settings default_model if None).
        client: Reusable Client instance. Creates a new one if None.
        timeout: Request timeout in seconds.

    Returns:
        Parsed JSON dict.

    Raises:
        RuntimeError: if the LLM call fails or the output is not valid JSON.
    """
    close_client = False
    if client is None:
        client = Client(timeout=timeout)
        close_client = True

    messages = [
        {"role": "system", "content": system_prompt},
        {"role": "user", "content": user_prompt},
    ]

    # ── Stream to temp file (append chunks as they arrive) ──
    try:
        with open(tmp_path, "w", encoding="utf-8") as f:
            def _on_chunk(chunk: str) -> None:
                f.write(chunk)
                f.flush()

            client.chat_completion(
                messages=messages, model=model, stream=True, on_chunk=_on_chunk
            )
    except Exception as e:
        # Fail fast — no retries. Clean up the temp file.
        try:
            os.remove(tmp_path)
        except OSError:
            pass
        if close_client:
            client.close()
        raise RuntimeError(f"LLM stream call failed: {e}") from e

    if close_client:
        client.close()

    # ── Read back and validate JSON ──
    with open(tmp_path, "r", encoding="utf-8") as f:
        text = f.read()

    # Extract ```json ... ``` block if present (same logic as call_llm_json)
    if "```json" in text:
        start = text.index("```json") + 7
        end = text.index("```", start) if "```" in text[start:] else len(text)
        text = text[start:end].strip()
    elif "```" in text:
        start = text.index("```") + 3
        end = text.index("```", start) if "```" in text[start:] else len(text)
        text = text[start:end].strip()

    try:
        result = json.loads(text)
    except json.JSONDecodeError as e:
        try:
            os.remove(tmp_path)
        except OSError:
            pass
        raise RuntimeError(f"LLM output is not valid JSON: {e}") from e

    return result


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
        # Try ``` ... ``` without language tag
        start = text.index("```") + 3
        end = text.index("```", start) if "```" in text[start:] else len(text)
        text = text[start:end].strip()

    return json.loads(text)
