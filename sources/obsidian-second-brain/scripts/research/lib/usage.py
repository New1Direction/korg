"""Soft cost tracking for Grok API calls. No blocking, just visibility."""

from datetime import datetime
from pathlib import Path
import json

from .config import USAGE_LOG

# Pricing per 1M tokens (as of 2026-04, https://docs.x.ai/docs/models)
GROK_PRICING = {
    "grok-4.20-reasoning": {"input": 3.00, "output": 15.00},
    "grok-4": {"input": 3.00, "output": 15.00},
    "grok-3": {"input": 2.00, "output": 10.00},
}


def estimate_cost(model: str, input_tokens: int, output_tokens: int) -> float:
    rates = GROK_PRICING.get(model, GROK_PRICING["grok-4.20-reasoning"])
    return (input_tokens / 1_000_000) * rates["input"] + (output_tokens / 1_000_000) * rates["output"]


def log_call(command: str, model: str, input_tokens: int, output_tokens: int, cost_usd: float, extra: dict | None = None) -> None:
    USAGE_LOG.parent.mkdir(parents=True, exist_ok=True)
    entry = {
        "ts": datetime.now().isoformat(timespec="seconds"),
        "command": command,
        "model": model,
        "input_tokens": input_tokens,
        "output_tokens": output_tokens,
        "cost_usd": round(cost_usd, 4),
    }
    if extra:
        entry.update(extra)
    with USAGE_LOG.open("a") as f:
        f.write(json.dumps(entry) + "\n")


def month_total() -> tuple[float, int]:
    if not USAGE_LOG.exists():
        return 0.0, 0
    now = datetime.now()
    month_prefix = now.strftime("%Y-%m")
    total = 0.0
    count = 0
    for line in USAGE_LOG.read_text().splitlines():
        try:
            entry = json.loads(line)
            if entry["ts"].startswith(month_prefix):
                total += entry["cost_usd"]
                count += 1
        except (json.JSONDecodeError, KeyError):
            continue
    return total, count
