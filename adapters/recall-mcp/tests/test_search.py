"""Tests for the RecallEngine — substring + semantic ranking."""

from __future__ import annotations

import json
from pathlib import Path

import pytest

from korg_recall_mcp.index import EventIndex
from korg_recall_mcp.search import (
    EmbeddingDependencyMissing,
    RecallEngine,
)


@pytest.fixture(scope="module")
def fastembed_available():
    pytest.importorskip("fastembed")
    return True


def _write_event(path: Path, seq: int, tool_name: str, args: dict, **extra) -> None:
    record = {
        "seq": seq,
        "source_agent": "agent:test",
        "tool_name": tool_name,
        "args": args,
        "result": extra.get("result", {}),
        "success": True,
        "duration_ms": 0,
    }
    with path.open("a") as f:
        f.write(json.dumps(record) + "\n")


def _seed_index(tmp_path: Path) -> EventIndex:
    f = tmp_path / "ledger.jsonl"
    _write_event(f, 1, "user_prompt", {"prompt": "how does the rust borrow checker prevent data races"})
    _write_event(f, 2, "llm_inference", {}, result={"text": "Rust enforces ownership at compile time."})
    _write_event(f, 3, "user_prompt", {"prompt": "best css flexbox alignment tricks"})
    _write_event(f, 4, "llm_inference", {}, result={"text": "Use align-items: center for vertical centering."})
    _write_event(f, 5, "user_prompt", {"prompt": "ownership and lifetimes in rust"})
    _write_event(f, 6, "Bash", {"command": "cargo test", "description": "run rust tests"}, result={"output": "5 passed"})
    idx = EventIndex.from_paths(f)
    idx.refresh()
    return idx


# ── Substring ─────────────────────────────────────────────────────────


def test_substring_returns_only_lines_containing_all_terms(tmp_path):
    idx = _seed_index(tmp_path)
    eng = RecallEngine(idx)
    matches = eng.search("rust ownership", mode="substring", top_n=10)
    # seq 5 contains both "rust" and "ownership"; seq 1 has rust but not ownership.
    found_seqs = sorted(m.event.seq for m in matches)
    assert 5 in found_seqs
    assert 1 not in found_seqs


def test_substring_is_case_insensitive(tmp_path):
    idx = _seed_index(tmp_path)
    eng = RecallEngine(idx)
    matches = eng.search("RUST", mode="substring", top_n=10)
    assert len(matches) >= 2  # multiple rust events


def test_substring_returns_empty_when_no_match(tmp_path):
    idx = _seed_index(tmp_path)
    eng = RecallEngine(idx)
    matches = eng.search("quantum chromodynamics", mode="substring", top_n=10)
    assert matches == []


def test_substring_respects_top_n(tmp_path):
    idx = _seed_index(tmp_path)
    eng = RecallEngine(idx)
    matches = eng.search("rust", mode="substring", top_n=2)
    assert len(matches) <= 2


def test_substring_via_field(tmp_path):
    idx = _seed_index(tmp_path)
    eng = RecallEngine(idx)
    matches = eng.search("rust", mode="substring", top_n=5)
    assert all(m.via == "substring" for m in matches)


def test_engine_records_last_mode(tmp_path):
    idx = _seed_index(tmp_path)
    eng = RecallEngine(idx)
    eng.search("rust", mode="substring")
    assert eng.last_mode == "substring"


def test_empty_query_returns_empty(tmp_path):
    idx = _seed_index(tmp_path)
    eng = RecallEngine(idx)
    assert eng.search("", mode="substring") == []
    assert eng.search("   ", mode="substring") == []


def test_tool_filter_restricts_results(tmp_path):
    idx = _seed_index(tmp_path)
    eng = RecallEngine(idx)
    # rust appears in user_prompt + llm_inference + Bash events;
    # filter to user_prompt only
    matches = eng.search("rust", mode="substring", top_n=10, tool_filter=["user_prompt"])
    assert all(m.event.tool_name == "user_prompt" for m in matches)


def test_refresh_called_automatically(tmp_path):
    """search() picks up newly-appended events without an explicit refresh()."""
    f = tmp_path / "ledger.jsonl"
    _write_event(f, 1, "user_prompt", {"prompt": "rust borrow checker"})
    idx = EventIndex.from_paths(f)
    eng = RecallEngine(idx)

    matches = eng.search("rust", mode="substring", top_n=5)
    assert len(matches) == 1

    _write_event(f, 2, "user_prompt", {"prompt": "rust lifetimes"})
    matches = eng.search("rust", mode="substring", top_n=5)
    assert len(matches) == 2


# ── Semantic (gated on fastembed) ─────────────────────────────────────


def test_semantic_finds_conceptually_related_event(tmp_path, fastembed_available):
    """The whole point: 'confused about borrowing' should find the rust
    borrow checker thread even though it has neither 'confused' nor
    'borrow' as a literal substring."""
    idx = _seed_index(tmp_path)
    eng = RecallEngine(idx)
    matches = eng.search("confused about borrowing", mode="semantic", min_score=0.30, top_n=3)
    assert len(matches) > 0
    # The borrow-checker user prompt should rank near the top
    top_text = matches[0].event.embed_text.lower()
    assert "rust" in top_text or "borrow" in top_text or "ownership" in top_text


def test_semantic_via_field(tmp_path, fastembed_available):
    idx = _seed_index(tmp_path)
    eng = RecallEngine(idx)
    matches = eng.search("rust", mode="semantic", min_score=0.30, top_n=5)
    assert all(m.via == "semantic" for m in matches)


def test_semantic_respects_min_score(tmp_path, fastembed_available):
    idx = _seed_index(tmp_path)
    eng = RecallEngine(idx)
    # Set min_score very high → most results filtered out
    matches = eng.search("rust", mode="semantic", min_score=0.99, top_n=10)
    # Score 0.99 vs 1.0 (perfect match) is extremely strict
    assert len(matches) < 5


def test_semantic_caches_event_embeddings(tmp_path, fastembed_available):
    idx = _seed_index(tmp_path)
    eng = RecallEngine(idx)
    eng.search("rust", mode="semantic")
    # All loaded events should have an embedding cached on the instance
    assert all(e.embedding is not None for e in idx.events)


def test_auto_mode_uses_semantic_when_available(tmp_path, fastembed_available):
    idx = _seed_index(tmp_path)
    eng = RecallEngine(idx)
    eng.search("rust", mode="auto")
    assert eng.last_mode == "semantic"


def test_auto_mode_falls_back_to_substring_without_fastembed(tmp_path, monkeypatch):
    """If fastembed isn't importable, auto mode silently switches to substring."""
    idx = _seed_index(tmp_path)
    eng = RecallEngine(idx)

    # Force the embedder to raise EmbeddingDependencyMissing
    class _MissingEmbedder:
        def embed_one(self, text):
            raise EmbeddingDependencyMissing("simulated missing dep")
        def embed_many(self, texts):
            raise EmbeddingDependencyMissing("simulated missing dep")

    eng.embedder = _MissingEmbedder()
    matches = eng.search("rust", mode="auto")
    assert eng.last_mode == "substring"
    # Substring matches should still be found
    assert any("rust" in m.event.embed_text.lower() for m in matches)


def test_semantic_raises_without_fastembed_when_explicit(tmp_path):
    """mode='semantic' must NOT silently fall back — it raises if the dep is missing."""
    idx = _seed_index(tmp_path)
    eng = RecallEngine(idx)

    class _MissingEmbedder:
        def embed_one(self, text):
            raise EmbeddingDependencyMissing("simulated missing dep")
        def embed_many(self, texts):
            raise EmbeddingDependencyMissing("simulated missing dep")

    eng.embedder = _MissingEmbedder()
    with pytest.raises(EmbeddingDependencyMissing):
        eng.search("anything", mode="semantic")
