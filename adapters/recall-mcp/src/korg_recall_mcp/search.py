"""Semantic + substring recall over an EventIndex.

Mirrors the KorgChat /recall engine design:
  - mode="auto"      uses semantic if fastembed is installed, else substring
  - mode="semantic"  requires fastembed; embeds query + events, cosine-ranks
  - mode="substring" pure AND-of-lowercased-terms, no dependencies

Embeddings are computed lazily and cached on the IndexedEvent instance.
A query embedding is computed fresh each call (cheap — single forward pass).

The default minimum cosine score (0.30) matches KorgChat's /recall floor.
For auto-context-style use (stricter), pass min_score=0.40 or higher.
"""

from __future__ import annotations

import math
from dataclasses import dataclass
from typing import Iterable, Literal, Optional

from korg_recall_mcp.index import EventIndex, IndexedEvent


Mode = Literal["auto", "semantic", "substring"]

DEFAULT_MIN_SCORE = 0.30
DEFAULT_TOP_N = 5
DEFAULT_EMBEDDING_MODEL = "BAAI/bge-small-en-v1.5"


class EmbeddingDependencyMissing(RuntimeError):
    """Raised when mode='semantic' is requested but fastembed isn't installed."""


@dataclass
class Match:
    event: IndexedEvent
    score: float
    via: Literal["semantic", "substring"]


# ── Embeddings ────────────────────────────────────────────────────────


class _LazyEmbedder:
    """Wraps fastembed.TextEmbedding — imported on first use so the
    semantic dep is genuinely optional."""

    def __init__(self, model_name: str = DEFAULT_EMBEDDING_MODEL) -> None:
        self.model_name = model_name
        self._model = None

    def _ensure_loaded(self) -> None:
        if self._model is not None:
            return
        try:
            from fastembed import TextEmbedding
        except ImportError as e:
            raise EmbeddingDependencyMissing(
                "fastembed is not installed. Run: pip install 'korg-recall-mcp[semantic]'"
            ) from e
        self._model = TextEmbedding(model_name=self.model_name)

    def embed_one(self, text: str) -> list[float]:
        self._ensure_loaded()
        # TextEmbedding.embed returns a generator over numpy arrays
        vec = next(iter(self._model.embed([text])))  # type: ignore[arg-type]
        return [float(x) for x in vec]

    def embed_many(self, texts: list[str]) -> list[list[float]]:
        self._ensure_loaded()
        vecs = list(self._model.embed(texts))  # type: ignore[arg-type]
        return [[float(x) for x in v] for v in vecs]


def _cosine(a: list[float], b: list[float]) -> float:
    if not a or not b or len(a) != len(b):
        return 0.0
    dot = sum(x * y for x, y in zip(a, b))
    na = math.sqrt(sum(x * x for x in a))
    nb = math.sqrt(sum(x * x for x in b))
    if na == 0 or nb == 0:
        return 0.0
    return dot / (na * nb)


# ── Engine ────────────────────────────────────────────────────────────


class RecallEngine:
    def __init__(
        self,
        index: EventIndex,
        embedder: Optional[_LazyEmbedder] = None,
    ) -> None:
        self.index = index
        self.embedder = embedder or _LazyEmbedder()
        self.last_mode: Literal["semantic", "substring"] | None = None

    # ── Public API ─────────────────────────────────────────────────────

    def search(
        self,
        query: str,
        *,
        mode: Mode = "auto",
        top_n: int = DEFAULT_TOP_N,
        min_score: float = DEFAULT_MIN_SCORE,
        tool_filter: Iterable[str] | None = None,
    ) -> list[Match]:
        if not query or not query.strip():
            return []
        # Make sure the index has loaded the latest events.
        self.index.refresh()

        events = list(self.index.events)
        if tool_filter:
            allowed = set(tool_filter)
            events = [e for e in events if e.tool_name in allowed]
        if not events:
            return []

        if mode == "substring":
            self.last_mode = "substring"
            return self._search_substring(query, events, top_n=top_n)
        if mode == "semantic":
            self.last_mode = "semantic"
            return self._search_semantic(query, events, top_n=top_n, min_score=min_score)
        # auto
        try:
            results = self._search_semantic(
                query, events, top_n=top_n, min_score=min_score
            )
            self.last_mode = "semantic"
            return results
        except EmbeddingDependencyMissing:
            self.last_mode = "substring"
            return self._search_substring(query, events, top_n=top_n)

    # ── Implementations ────────────────────────────────────────────────

    def _search_substring(
        self,
        query: str,
        events: list[IndexedEvent],
        *,
        top_n: int,
    ) -> list[Match]:
        terms = [t for t in query.lower().split() if t]
        out: list[Match] = []
        for ev in events:
            haystack = ev.embed_text.lower()
            if all(t in haystack for t in terms):
                # Score is 1.0 minus a small penalty for length — favors
                # concise matches, which usually correlate with signal.
                score = 1.0 - min(0.5, len(haystack) / 2000.0)
                out.append(Match(event=ev, score=score, via="substring"))
        out.sort(key=lambda m: m.score, reverse=True)
        return out[:top_n]

    def _search_semantic(
        self,
        query: str,
        events: list[IndexedEvent],
        *,
        top_n: int,
        min_score: float,
    ) -> list[Match]:
        # Embed any events that haven't been embedded yet.
        unembedded = [e for e in events if e.embedding is None]
        if unembedded:
            vecs = self.embedder.embed_many([e.embed_text for e in unembedded])
            for ev, vec in zip(unembedded, vecs):
                ev.embedding = vec

        qvec = self.embedder.embed_one(query)
        scored: list[Match] = []
        for ev in events:
            if ev.embedding is None:
                continue
            s = _cosine(qvec, ev.embedding)
            if s >= min_score:
                scored.append(Match(event=ev, score=s, via="semantic"))
        scored.sort(key=lambda m: m.score, reverse=True)
        return scored[:top_n]
