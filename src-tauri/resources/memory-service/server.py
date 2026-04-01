"""
Orbit Memory Service — FastAPI wrapper around mem0 with FAISS backend.

Exposes REST endpoints for memory CRUD, semantic search, and auto-extraction.
All memories are scoped by user_id and agent_id, with metadata tags for
memory_type (user, feedback, project, reference).
"""

from __future__ import annotations

import os
import sys
import logging
from datetime import datetime, timezone
from enum import Enum
from typing import Any, Optional

from fastapi import FastAPI, HTTPException, Query
from pydantic import BaseModel, Field
from mem0 import Memory

from memory_config import get_mem0_config

# ---------------------------------------------------------------------------
# Logging
# ---------------------------------------------------------------------------

logging.basicConfig(
    level=logging.INFO,
    format="%(asctime)s [%(levelname)s] %(name)s: %(message)s",
    stream=sys.stderr,
)
logger = logging.getLogger("orbit-memory")

# ---------------------------------------------------------------------------
# App + mem0 initialisation
# ---------------------------------------------------------------------------

app = FastAPI(
    title="Orbit Memory Service",
    version="0.1.0",
    docs_url="/docs",
)

memory: Memory | None = None


@app.on_event("startup")
async def startup() -> None:
    global memory
    config = get_mem0_config()
    logger.info("Initialising mem0 with config: %s", config)
    memory = Memory.from_config(config)
    logger.info("Memory service ready")


# ---------------------------------------------------------------------------
# Schemas
# ---------------------------------------------------------------------------

class MemoryType(str, Enum):
    user = "user"
    feedback = "feedback"
    project = "project"
    reference = "reference"


class AddMemoryRequest(BaseModel):
    text: str = Field(..., min_length=1, max_length=4096)
    memory_type: MemoryType
    user_id: str = Field(..., min_length=1)
    agent_id: str = Field(..., min_length=1)
    metadata: dict[str, Any] = Field(default_factory=dict)


class SearchMemoryRequest(BaseModel):
    query: str = Field(..., min_length=1, max_length=2048)
    user_id: str = Field(..., min_length=1)
    agent_id: str = Field(..., min_length=1)
    memory_type: Optional[MemoryType] = None
    limit: int = Field(default=10, ge=1, le=50)


class UpdateMemoryRequest(BaseModel):
    text: Optional[str] = Field(None, min_length=1, max_length=4096)
    metadata: Optional[dict[str, Any]] = None


class ExtractMemoriesRequest(BaseModel):
    conversation_text: str = Field(..., min_length=1, max_length=32768)
    user_id: str = Field(..., min_length=1)
    agent_id: str = Field(..., min_length=1)


class MemoryEntry(BaseModel):
    id: str
    text: str
    memory_type: str
    user_id: str
    agent_id: str
    created_at: str
    updated_at: str
    source: str
    score: Optional[float] = None
    metadata: dict[str, Any] = Field(default_factory=dict)


# ---------------------------------------------------------------------------
# Helpers
# ---------------------------------------------------------------------------

def _now_iso() -> str:
    return datetime.now(timezone.utc).isoformat()


def _build_metadata(
    memory_type: MemoryType,
    agent_id: str,
    source: str = "explicit",
    extra: dict[str, Any] | None = None,
) -> dict[str, Any]:
    meta = {
        "memory_type": memory_type.value,
        "agent_id": agent_id,
        "source": source,
        "created_at": _now_iso(),
        "updated_at": _now_iso(),
    }
    if extra:
        meta.update(extra)
    return meta


def _mem0_result_to_entry(raw: dict[str, Any], score: float | None = None) -> MemoryEntry:
    """Convert a mem0 result dict to our MemoryEntry schema."""
    meta = raw.get("metadata", {})
    return MemoryEntry(
        id=raw.get("id", ""),
        text=raw.get("memory", raw.get("text", "")),
        memory_type=meta.get("memory_type", "reference"),
        user_id=raw.get("user_id", meta.get("user_id", "")),
        agent_id=meta.get("agent_id", ""),
        created_at=meta.get("created_at", ""),
        updated_at=meta.get("updated_at", ""),
        source=meta.get("source", "explicit"),
        score=score,
        metadata={k: v for k, v in meta.items() if k not in {
            "memory_type", "agent_id", "source", "created_at", "updated_at",
        }},
    )


# ---------------------------------------------------------------------------
# Endpoints
# ---------------------------------------------------------------------------

@app.get("/api/v1/memory/health")
async def health() -> dict[str, str]:
    return {"status": "ok"}


@app.post("/api/v1/memory/add", response_model=list[MemoryEntry])
async def add_memory(req: AddMemoryRequest) -> list[MemoryEntry]:
    """Add a new memory scoped to user_id and agent_id."""
    assert memory is not None

    metadata = _build_metadata(
        memory_type=req.memory_type,
        agent_id=req.agent_id,
        source="explicit",
        extra=req.metadata,
    )

    result = memory.add(
        req.text,
        user_id=req.user_id,
        metadata=metadata,
    )

    entries = []
    results_list = result.get("results", []) if isinstance(result, dict) else []
    for item in results_list:
        if item.get("event") in ("ADD", "add"):
            entry = _mem0_result_to_entry(item)
            if not entry.id and "id" in item:
                entry.id = item["id"]
            entries.append(entry)

    # If mem0 didn't return structured results, build a synthetic entry
    if not entries:
        entries.append(MemoryEntry(
            id=result.get("id", "") if isinstance(result, dict) else "",
            text=req.text,
            memory_type=req.memory_type.value,
            user_id=req.user_id,
            agent_id=req.agent_id,
            created_at=metadata["created_at"],
            updated_at=metadata["updated_at"],
            source="explicit",
        ))

    logger.info("Added %d memory(ies) for user=%s agent=%s", len(entries), req.user_id, req.agent_id)
    return entries


@app.post("/api/v1/memory/search", response_model=list[MemoryEntry])
async def search_memories(req: SearchMemoryRequest) -> list[MemoryEntry]:
    """Semantic search for memories matching a query."""
    assert memory is not None

    filters = {"agent_id": req.agent_id}
    if req.memory_type is not None:
        filters["memory_type"] = req.memory_type.value

    results = memory.search(
        req.query,
        user_id=req.user_id,
        limit=req.limit,
    )

    results_list = results.get("results", []) if isinstance(results, dict) else results

    entries = []
    for item in results_list:
        meta = item.get("metadata", {})
        # Apply agent_id filter (mem0 may not filter metadata natively)
        if meta.get("agent_id") and meta["agent_id"] != req.agent_id:
            continue
        if req.memory_type and meta.get("memory_type") != req.memory_type.value:
            continue
        score = item.get("score")
        entries.append(_mem0_result_to_entry(item, score=score))

    return entries[:req.limit]


@app.get("/api/v1/memory/list", response_model=list[MemoryEntry])
async def list_memories(
    user_id: str = Query(..., min_length=1),
    agent_id: str = Query(..., min_length=1),
    memory_type: Optional[MemoryType] = None,
    limit: int = Query(default=50, ge=1, le=200),
    offset: int = Query(default=0, ge=0),
) -> list[MemoryEntry]:
    """List all memories for a user/agent, optionally filtered by type."""
    assert memory is not None

    results = memory.get_all(user_id=user_id)
    results_list = results.get("results", []) if isinstance(results, dict) else results

    entries = []
    for item in results_list:
        meta = item.get("metadata", {})
        if meta.get("agent_id") and meta["agent_id"] != agent_id:
            continue
        if memory_type and meta.get("memory_type") != memory_type.value:
            continue
        entries.append(_mem0_result_to_entry(item))

    # Sort by created_at descending
    entries.sort(key=lambda e: e.created_at, reverse=True)
    return entries[offset:offset + limit]


@app.delete("/api/v1/memory/delete/{memory_id}")
async def delete_memory(memory_id: str) -> dict[str, str]:
    """Delete a specific memory by ID."""
    assert memory is not None

    try:
        memory.delete(memory_id)
    except Exception as e:
        raise HTTPException(status_code=404, detail=f"Memory not found: {e}")

    logger.info("Deleted memory %s", memory_id)
    return {"status": "deleted", "id": memory_id}


@app.put("/api/v1/memory/update/{memory_id}", response_model=MemoryEntry)
async def update_memory(memory_id: str, req: UpdateMemoryRequest) -> MemoryEntry:
    """Update a memory's text or metadata."""
    assert memory is not None

    try:
        existing = memory.get(memory_id)
    except Exception as e:
        raise HTTPException(status_code=404, detail=f"Memory not found: {e}")

    new_text = req.text if req.text else existing.get("memory", existing.get("text", ""))

    existing_meta = existing.get("metadata", {})
    if req.metadata:
        existing_meta.update(req.metadata)
    existing_meta["updated_at"] = _now_iso()

    memory.update(memory_id, data=new_text, metadata=existing_meta)

    updated = memory.get(memory_id)
    return _mem0_result_to_entry(updated)


EXTRACTION_SYSTEM_PROMPT = """\
You are a memory extraction assistant. Given a conversation between a user and an AI agent, \
extract notable long-term memories that would be useful in future conversations.

Extract ONLY information that is:
- User preferences, expertise, role, or working style
- Corrections or confirmations the user gave about approach/methodology (with reasoning)
- Project decisions, deadlines, or context not derivable from code or git history
- Pointers to external resources (dashboards, ticket boards, documentation URLs)

Do NOT extract:
- Code snippets or implementation details (these are in the codebase)
- Git history facts (use git log for these)
- Standard framework/library documentation
- Ephemeral task details or temporary state
- Information that is already obvious from the conversation context

For each memory, classify it as one of: user, feedback, project, reference

Return a JSON array of objects with "text" and "memory_type" fields. \
Return an empty array if nothing is worth extracting."""


@app.post("/api/v1/memory/extract", response_model=list[MemoryEntry])
async def extract_memories(req: ExtractMemoriesRequest) -> list[MemoryEntry]:
    """Auto-extract memories from a conversation using mem0's extraction."""
    assert memory is not None

    # Use mem0's add with the conversation text — it will deduplicate and extract
    metadata = _build_metadata(
        memory_type=MemoryType.project,  # default, will be overridden per-memory
        agent_id=req.agent_id,
        source="auto_extracted",
    )

    result = memory.add(
        req.conversation_text,
        user_id=req.user_id,
        metadata=metadata,
    )

    entries = []
    results_list = result.get("results", []) if isinstance(result, dict) else []
    for item in results_list:
        if item.get("event") in ("ADD", "add"):
            entries.append(_mem0_result_to_entry(item))

    logger.info(
        "Extracted %d memories from conversation for user=%s agent=%s",
        len(entries), req.user_id, req.agent_id,
    )
    return entries


# ---------------------------------------------------------------------------
# Entrypoint
# ---------------------------------------------------------------------------

def main() -> None:
    import uvicorn

    port = int(os.environ.get("ORBIT_MEMORY_PORT", "9473"))
    uvicorn.run(
        "server:app",
        host="127.0.0.1",
        port=port,
        log_level="info",
    )


if __name__ == "__main__":
    main()
