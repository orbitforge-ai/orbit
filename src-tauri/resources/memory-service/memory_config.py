"""
mem0 configuration for Orbit's self-hosted memory service.

Uses FAISS as the vector store backend. Data is persisted at
~/.orbit/memory-service/data/. Designed to be swappable to managed
mem0 or a different vector backend by changing this config.
"""

import os
from pathlib import Path

DATA_DIR = Path(os.environ.get(
    "ORBIT_MEMORY_DATA_DIR",
    Path.home() / ".orbit" / "memory-service" / "data",
))
DATA_DIR.mkdir(parents=True, exist_ok=True)


def get_mem0_config() -> dict:
    """Return the mem0 configuration dictionary."""
    return {
        "vector_store": {
            "provider": "faiss",
            "config": {
                "embedding_dims": 1536,
                "path": str(DATA_DIR / "faiss_index"),
            },
        },
        "version": "v1.1",
    }
