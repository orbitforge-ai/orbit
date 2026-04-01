"""
Orbit Memory MCP Server

Exposes the Orbit memory service as an MCP server over stdio.
Delegates all operations to the local REST API (server.py) rather than
accessing mem0 directly, so both interfaces share a single consistent state.

Usage:
    uv run python mcp_server.py [--port 9473]

Tools exposed:
    memory_add       — Add a new memory
    memory_search    — Semantic search across memories
    memory_list      — List memories (optionally filtered by type)
    memory_delete    — Delete a memory by ID
    memory_update    — Update a memory's text
"""

from __future__ import annotations

import argparse
import asyncio
import json
import sys
from typing import Any

import httpx
from mcp.server import Server
from mcp.server.stdio import stdio_server
from mcp import types

# ---------------------------------------------------------------------------
# CLI args
# ---------------------------------------------------------------------------

parser = argparse.ArgumentParser(description="Orbit Memory MCP Server")
parser.add_argument("--port", type=int, default=9473, help="REST API port (default: 9473)")
args, _ = parser.parse_known_args()

BASE_URL = f"http://127.0.0.1:{args.port}/api/v1/memory"

# ---------------------------------------------------------------------------
# HTTP helpers
# ---------------------------------------------------------------------------

async def _post(path: str, body: dict[str, Any]) -> Any:
    async with httpx.AsyncClient(timeout=30) as client:
        resp = await client.post(f"{BASE_URL}{path}", json=body)
        resp.raise_for_status()
        return resp.json()


async def _get(path: str, params: dict[str, Any] | None = None) -> Any:
    async with httpx.AsyncClient(timeout=30) as client:
        resp = await client.get(f"{BASE_URL}{path}", params=params)
        resp.raise_for_status()
        return resp.json()


async def _delete(path: str) -> None:
    async with httpx.AsyncClient(timeout=30) as client:
        resp = await client.delete(f"{BASE_URL}{path}")
        resp.raise_for_status()


async def _put(path: str, body: dict[str, Any]) -> Any:
    async with httpx.AsyncClient(timeout=30) as client:
        resp = await client.put(f"{BASE_URL}{path}", json=body)
        resp.raise_for_status()
        return resp.json()


def _format_entries(entries: list[dict[str, Any]]) -> str:
    if not entries:
        return "No memories found."
    lines = []
    for e in entries:
        score = f" (score: {e['score']:.2f})" if e.get("score") is not None else ""
        lines.append(f"[{e['id']}] [{e['memory_type']}] {e['text']}{score}")
    return "\n".join(lines)


# ---------------------------------------------------------------------------
# MCP server
# ---------------------------------------------------------------------------

server = Server("orbit-memory")


@server.list_tools()
async def list_tools() -> list[types.Tool]:
    return [
        types.Tool(
            name="memory_add",
            description=(
                "Save a piece of information to Orbit's long-term memory. "
                "Use this to persist facts, preferences, feedback, or project context."
            ),
            inputSchema={
                "type": "object",
                "properties": {
                    "text": {"type": "string", "description": "The information to remember"},
                    "memory_type": {
                        "type": "string",
                        "enum": ["user", "feedback", "project", "reference"],
                        "description": "Category of the memory",
                    },
                    "user_id": {
                        "type": "string",
                        "description": "User scope (default: default_user)",
                    },
                    "agent_id": {
                        "type": "string",
                        "description": "Agent scope",
                    },
                },
                "required": ["text", "memory_type", "agent_id"],
            },
        ),
        types.Tool(
            name="memory_search",
            description="Semantic search across Orbit memories. Returns the most relevant results.",
            inputSchema={
                "type": "object",
                "properties": {
                    "query": {"type": "string", "description": "What to search for"},
                    "user_id": {"type": "string", "description": "User scope (default: default_user)"},
                    "agent_id": {"type": "string", "description": "Agent scope"},
                    "memory_type": {
                        "type": "string",
                        "enum": ["user", "feedback", "project", "reference"],
                        "description": "Optional: filter by memory type",
                    },
                    "limit": {
                        "type": "integer",
                        "description": "Max results (default: 10, max: 50)",
                    },
                },
                "required": ["query", "agent_id"],
            },
        ),
        types.Tool(
            name="memory_list",
            description="List memories for a user/agent, optionally filtered by type.",
            inputSchema={
                "type": "object",
                "properties": {
                    "user_id": {"type": "string", "description": "User scope (default: default_user)"},
                    "agent_id": {"type": "string", "description": "Agent scope"},
                    "memory_type": {
                        "type": "string",
                        "enum": ["user", "feedback", "project", "reference"],
                        "description": "Optional: filter by memory type",
                    },
                    "limit": {"type": "integer", "description": "Max results (default: 50)"},
                    "offset": {"type": "integer", "description": "Pagination offset (default: 0)"},
                },
                "required": ["agent_id"],
            },
        ),
        types.Tool(
            name="memory_delete",
            description="Delete a memory by its ID.",
            inputSchema={
                "type": "object",
                "properties": {
                    "memory_id": {"type": "string", "description": "The ID of the memory to delete"},
                },
                "required": ["memory_id"],
            },
        ),
        types.Tool(
            name="memory_update",
            description="Update the text of an existing memory.",
            inputSchema={
                "type": "object",
                "properties": {
                    "memory_id": {"type": "string", "description": "The ID of the memory to update"},
                    "text": {"type": "string", "description": "New text for the memory"},
                },
                "required": ["memory_id", "text"],
            },
        ),
    ]


@server.call_tool()
async def call_tool(name: str, arguments: dict[str, Any]) -> list[types.TextContent]:
    try:
        if name == "memory_add":
            entries = await _post("/add", {
                "text": arguments["text"],
                "memory_type": arguments["memory_type"],
                "user_id": arguments.get("user_id", "default_user"),
                "agent_id": arguments["agent_id"],
            })
            count = len(entries) if isinstance(entries, list) else 1
            result = f"Saved memory ({count} entr{'ies' if count != 1 else 'y'} stored)."

        elif name == "memory_search":
            entries = await _post("/search", {
                "query": arguments["query"],
                "user_id": arguments.get("user_id", "default_user"),
                "agent_id": arguments["agent_id"],
                "memory_type": arguments.get("memory_type"),
                "limit": arguments.get("limit", 10),
            })
            result = _format_entries(entries)

        elif name == "memory_list":
            params: dict[str, Any] = {
                "user_id": arguments.get("user_id", "default_user"),
                "agent_id": arguments["agent_id"],
                "limit": arguments.get("limit", 50),
                "offset": arguments.get("offset", 0),
            }
            if "memory_type" in arguments:
                params["memory_type"] = arguments["memory_type"]
            entries = await _get("/list", params)
            result = _format_entries(entries)

        elif name == "memory_delete":
            await _delete(f"/delete/{arguments['memory_id']}")
            result = f"Deleted memory {arguments['memory_id']}."

        elif name == "memory_update":
            entry = await _put(f"/update/{arguments['memory_id']}", {"text": arguments["text"]})
            result = f"Updated memory {entry['id']}: {entry['text']}"

        else:
            result = f"Unknown tool: {name}"

    except httpx.HTTPStatusError as exc:
        result = f"Error {exc.response.status_code}: {exc.response.text}"
    except httpx.ConnectError:
        result = "Memory service is not reachable. Is the Orbit app running?"
    except Exception as exc:  # noqa: BLE001
        result = f"Unexpected error: {exc}"

    return [types.TextContent(type="text", text=result)]


# ---------------------------------------------------------------------------
# Entry point
# ---------------------------------------------------------------------------

async def main() -> None:
    async with stdio_server() as (read_stream, write_stream):
        await server.run(
            read_stream,
            write_stream,
            server.create_initialization_options(),
        )


if __name__ == "__main__":
    asyncio.run(main())
