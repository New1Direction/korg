"""
Operation abstraction for cli-anything-bun.

Provides a clean, deterministic way to execute finite/transactional
operations (package install, script run, etc.) while automatically
handling:

- op_id generation and correlation
- Event stream emission (started + intermediate + result)
- Success and error paths
- Integration with the stable event envelope

This is intentionally minimal for Phase 1.
"""

from __future__ import annotations

import uuid
from contextlib import contextmanager
from typing import Any, Optional, Dict, Iterator

from .events import (
    BunEvent,
    EventLevel,
    emit,
    emit_result,
)


class Operation:
    """
    Represents a single finite operation (e.g. "package.install").

    Usage as context manager (recommended):

        with Operation("package.install", {"packages": ["hono"]}) as op:
            op.emit("resolved", {"count": 47})
            # ... do work ...
            op.set_result(packages_added=3)

    On successful exit:
        - Emits a final result with ok=true

    On exception:
        - Emits an error event
        - Emits a final result with ok=false + error details
        - Re-raises the original exception
    """

    def __init__(
        self,
        name: str,
        initial_data: Optional[Dict[str, Any]] = None,
        session_id: Optional[str] = None,
    ):
        self.name = name
        self.op_id = str(uuid.uuid4())
        self.session_id = session_id
        self._result_data: Dict[str, Any] = initial_data or {}
        self._started = False

    def __enter__(self) -> Operation:
        self._started = True
        emit(
            f"{self.name}.started",
            self._result_data.copy(),
            level=EventLevel.INFO,
            op_id=self.op_id,
            session_id=self.session_id,
        )
        return self

    def __exit__(self, exc_type, exc_val, exc_tb) -> bool:
        if exc_type is None:
            # Success path
            self._emit_result(ok=True)
        else:
            # Error path
            error_data = self._build_error_data(exc_val)
            emit(
                f"{self.name}.failed",
                error_data,
                level=EventLevel.ERROR,
                op_id=self.op_id,
                session_id=self.session_id,
            )
            self._emit_result(ok=False, error=error_data)

            # Do not suppress the exception
            return False

        return False

    def emit(
        self,
        event_suffix: str,
        data: Optional[Dict[str, Any]] = None,
        level: EventLevel = EventLevel.INFO,
    ) -> BunEvent:
        """
        Emit an event that is automatically correlated with this operation.
        """
        full_event = (
            event_suffix
            if event_suffix.startswith(self.name)
            else f"{self.name}.{event_suffix}"
        )

        return emit(
            full_event,
            data or {},
            level=level,
            op_id=self.op_id,
            session_id=self.session_id,
        )

    def set_result(self, **kwargs: Any) -> None:
        """Merge additional data into the final result."""
        self._result_data.update(kwargs)

    def _emit_result(self, ok: bool, error: Optional[Dict[str, Any]] = None) -> None:
        result: Dict[str, Any] = {
            "ok": ok,
            "operation": self.name,
            **self._result_data,
        }
        if error is not None:
            result["error"] = error

        emit_result(result)

    def _build_error_data(self, exc: BaseException) -> Dict[str, Any]:
        """Convert an exception into our normalized error model (Phase 1 version)."""
        return {
            "type": "runtime_error",
            "recoverable": False,
            "retryable": False,
            "message": str(exc),
            "exception_type": exc.__class__.__name__,
        }


# Convenience context manager for simple one-liner usage
@contextmanager
def operation(
    name: str,
    initial_data: Optional[Dict[str, Any]] = None,
    session_id: Optional[str] = None,
) -> Iterator[Operation]:
    """
    Context manager version of Operation for convenience.

    Example:
        with operation("package.add", {"package": "hono"}) as op:
            ...
    """
    op = Operation(name, initial_data, session_id)
    with op:
        yield op
