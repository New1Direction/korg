"""Hybrid Logical Clock — mirrors crates/korg-registry/src/log.rs HlcTimestamp."""
from __future__ import annotations

from dataclasses import dataclass


@dataclass(frozen=True)
class Hlc:
    physical: int
    logical: int
    actor_id: int = 1

    def tick(self, wall_clock_ms: int) -> "Hlc":
        new_physical = max(wall_clock_ms, self.physical)
        new_logical = self.logical + 1 if new_physical == self.physical else 0
        return Hlc(new_physical, new_logical, self.actor_id)

    def as_dict(self) -> dict:
        return {
            "physical": self.physical,
            "logical": self.logical,
            "actor_id": self.actor_id,
        }
