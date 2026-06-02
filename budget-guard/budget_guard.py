#!/usr/bin/env python3
"""Budget Guard — Automatic cost-aware model throttling for Codex CLI.

Reads `.budget.toml` from the project or home directory, tracks token spend
against daily/weekly/monthly budgets, and auto-switches from the primary
model to the economy model when thresholds are crossed.

Usage:
    budget-guard status          # Show current budget state
    budget-guard log             # Show recent spend log
    budget-guard record <cost>   # Record a command's cost (called by hooks)
    budget-guard check           # Return recommended model (primary or economy)
"""

from __future__ import annotations

import argparse
import json
import os
import sys
import time
from dataclasses import dataclass, field, asdict
from datetime import datetime, timezone
from pathlib import Path
from typing import Optional

try:
    import tomllib
except ModuleNotFoundError:
    import tomli as tomllib


# ── Data Model ──────────────────────────────────────────────────────

@dataclass
class BudgetConfig:
    daily: float = 10.00
    weekly: float = 50.00
    monthly: float = 200.00
    primary_model: str = "gpt-4o"
    economy_model: str = "gpt-4o-mini"
    trigger_at: float = 0.85
    cooldown_hours: float = 2.0
    spend_log: str = ".codex/budget-spend.jsonl"
    verbose: bool = True

    @classmethod
    def load(cls, path: Optional[Path] = None) -> "BudgetConfig":
        """Load config from `.budget.toml`. Searches upward from cwd if no path given."""
        if path is None:
            path = cls._find_toml()

        if path is None or not path.exists():
            return cls()

        with open(path, "rb") as f:
            data = tomllib.load(f)

        c = cls()
        if "daily" in data:
            c.daily = data["daily"].get("limit", c.daily)
        if "weekly" in data:
            c.weekly = data["weekly"].get("limit", c.weekly)
        if "monthly" in data:
            c.monthly = data["monthly"].get("limit", c.monthly)
        if "models" in data:
            c.primary_model = data["models"].get("primary", c.primary_model)
            c.economy_model = data["models"].get("economy", c.economy_model)
        if "throttle" in data:
            c.trigger_at = data["throttle"].get("trigger_at", c.trigger_at)
            c.cooldown_hours = data["throttle"].get("cooldown_hours", c.cooldown_hours)
        if "logging" in data:
            c.spend_log = data["logging"].get("spend_log", c.spend_log)
            c.verbose = data["logging"].get("verbose", c.verbose)
        return c

    @staticmethod
    def _find_toml() -> Optional[Path]:
        cwd = Path.cwd()
        for parent in [cwd, *cwd.parents]:
            candidate = parent / ".budget.toml"
            if candidate.exists():
                return candidate
        # Fallback: home directory
        home = Path.home() / ".codex" / ".budget.toml"
        if home.exists():
            return home
        return None


@dataclass
class SpendEntry:
    timestamp: float
    cost: float
    model: str
    command: str
    tokens_in: int = 0
    tokens_out: int = 0

    @property
    def datetime_utc(self) -> str:
        return datetime.fromtimestamp(self.timestamp, tz=timezone.utc).strftime(
            "%Y-%m-%d %H:%M:%S UTC"
        )

    def to_json(self) -> dict:
        return asdict(self)

    @classmethod
    def from_json(cls, data: dict) -> "SpendEntry":
        return cls(
            timestamp=data["timestamp"],
            cost=data["cost"],
            model=data["model"],
            command=data["command"],
            tokens_in=data.get("tokens_in", 0),
            tokens_out=data.get("tokens_out", 0),
        )


@dataclass
class BudgetState:
    daily_spend: float = 0.0
    weekly_spend: float = 0.0
    monthly_spend: float = 0.0
    daily_limit: float = 10.00
    weekly_limit: float = 50.00
    monthly_limit: float = 200.00
    primary_model: str = "gpt-4o"
    economy_model: str = "gpt-4o-mini"
    trigger_at: float = 0.85
    cooldown_hours: float = 2.0
    last_throttled_at: Optional[float] = None
    entries: list[SpendEntry] = field(default_factory=list)

    def load(self, config: BudgetConfig) -> None:
        self.daily_limit = config.daily
        self.weekly_limit = config.weekly
        self.monthly_limit = config.monthly
        self.primary_model = config.primary_model
        self.economy_model = config.economy_model
        self.trigger_at = config.trigger_at
        self.cooldown_hours = config.cooldown_hours
        self._recompute_spend(config)

    def _recompute_spend(self, config: BudgetConfig) -> None:
        log_path = self._resolve_log_path(config.spend_log)
        if not log_path.exists():
            return

        now = time.time()
        day_ago = now - 86400
        week_ago = now - 604800
        month_ago = now - 2592000

        entries = []
        with open(log_path) as f:
            for line in f:
                line = line.strip()
                if not line:
                    continue
                try:
                    data = json.loads(line)
                    entry = SpendEntry.from_json(data)
                    entries.append(entry)
                except (json.JSONDecodeError, KeyError):
                    continue

        self.entries = entries

        self.daily_spend = sum(e.cost for e in entries if day_ago <= e.timestamp <= now)
        self.weekly_spend = sum(e.cost for e in entries if week_ago <= e.timestamp <= now)
        self.monthly_spend = sum(e.cost for e in entries if month_ago <= e.timestamp <= now)

    @staticmethod
    def _resolve_log_path(spend_log: str) -> Path:
        p = Path(spend_log)
        if p.is_absolute():
            return p
        # Relative to codex home or cwd
        codex_home = Path(os.environ.get("CODEX_HOME", Path.home() / ".codex"))
        candidate = codex_home / spend_log
        if candidate.parent.exists():
            return candidate
        return Path.cwd() / spend_log

    @property
    def daily_pct(self) -> float:
        if self.daily_limit == 0:
            return 0.0
        return round(self.daily_spend / self.daily_limit, 3)

    @property
    def weekly_pct(self) -> float:
        if self.weekly_limit == 0:
            return 0.0
        return round(self.weekly_spend / self.weekly_limit, 3)

    @property
    def monthly_pct(self) -> float:
        if self.monthly_limit == 0:
            return 0.0
        return round(self.monthly_spend / self.monthly_limit, 3)

    @property
    def recommended_model(self) -> str:
        """Return the model to use: primary unless throttled."""
        if self._should_throttle():
            return self.economy_model
        return self.primary_model

    @property
    def is_throttled(self) -> bool:
        return self.recommended_model != self.primary_model

    def _should_throttle(self) -> bool:
        # Check if any budget window is past the trigger threshold
        over_threshold = (
            self.daily_pct >= self.trigger_at
            or self.weekly_pct >= self.trigger_at
            or self.monthly_pct >= self.trigger_at
        )
        if not over_threshold:
            return False

        # Check cooldown: if we already throttled and cooldown hasn't expired,
        # stay on economy
        if self.last_throttled_at is not None:
            elapsed = time.time() - self.last_throttled_at
            if elapsed < self.cooldown_hours * 3600:
                return True

        return over_threshold

    def record(self, entry: SpendEntry, config: BudgetConfig) -> bool:
        """Record a spend entry. Returns True if now throttled."""
        log_path = self._resolve_log_path(config.spend_log)
        log_path.parent.mkdir(parents=True, exist_ok=True)

        with open(log_path, "a") as f:
            f.write(json.dumps(entry.to_json()) + "\n")

        self.entries.append(entry)
        self.daily_spend += entry.cost
        self.weekly_spend += entry.cost
        self.monthly_spend += entry.cost

        if self._should_throttle():
            self.last_throttled_at = time.time()
            return True
        return False

    def status_report(self) -> str:
        lines = []
        now = datetime.now(timezone.utc).strftime("%Y-%m-%d %H:%M:%S UTC")
        lines.append(f"Budget Guard Status — {now}")
        lines.append("")
        lines.append(f"  Daily:   ${self.daily_spend:.2f} / ${self.daily_limit:.2f}  ({self.daily_pct:.1%})")
        lines.append(f"  Weekly:  ${self.weekly_spend:.2f} / ${self.weekly_limit:.2f}  ({self.weekly_pct:.1%})")
        lines.append(f"  Monthly: ${self.monthly_spend:.2f} / ${self.monthly_limit:.2f}  ({self.monthly_pct:.1%})")
        lines.append("")
        lines.append(f"  Primary model: {self.primary_model}")
        lines.append(f"  Economy model: {self.economy_model}")
        if self.is_throttled:
            lines.append(f"  ▶ Throttled to {self.economy_model}")
        else:
            lines.append(f"  ▶ Using {self.primary_model}")
        lines.append("")
        lines.append(f"  Last {len(self.entries)} entries:")
        for e in reversed(self.entries[-5:]):
            lines.append(
                f"    {e.datetime_utc}  ${e.cost:.4f}  {e.model:16s}  {e.command[:50]}"
            )
        return "\n".join(lines)


# ── CLI ─────────────────────────────────────────────────────────────

def cmd_status(_args) -> None:
    config = BudgetConfig.load()
    state = BudgetState()
    state.load(config)
    print(state.status_report())


def cmd_log(args) -> None:
    config = BudgetConfig.load()
    state = BudgetState()
    state.load(config)
    limit = args.limit if args.limit else len(state.entries)
    for e in reversed(state.entries[-limit:]):
        record = {
            "time": e.datetime_utc,
            "cost": round(e.cost, 4),
            "model": e.model,
            "command": e.command,
            "tokens_in": e.tokens_in,
            "tokens_out": e.tokens_out,
        }
        print(json.dumps(record))


def cmd_record(args) -> None:
    config = BudgetConfig.load()
    state = BudgetState()
    state.load(config)
    entry = SpendEntry(
        timestamp=time.time(),
        cost=args.cost,
        model=args.model,
        command=args.command,
        tokens_in=args.tokens_in,
        tokens_out=args.tokens_out,
    )
    throttled = state.record(entry, config)
    if throttled:
        print(json.dumps({"throttled": True, "recommended_model": state.economy_model}))
    else:
        print(json.dumps({"throttled": False, "recommended_model": state.primary_model}))


def cmd_check(_args) -> None:
    config = BudgetConfig.load()
    state = BudgetState()
    state.load(config)
    result = {
        "recommended_model": state.recommended_model,
        "is_throttled": state.is_throttled,
        "daily": {"spend": state.daily_spend, "limit": state.daily_limit, "pct": state.daily_pct},
        "weekly": {"spend": state.weekly_spend, "limit": state.weekly_limit, "pct": state.weekly_pct},
        "monthly": {"spend": state.monthly_spend, "limit": state.monthly_limit, "pct": state.monthly_pct},
    }
    print(json.dumps(result, indent=2))


def cmd_demo(_args) -> None:
    """Run a simulation showing budget guard behavior over a few days."""
    config = BudgetConfig(daily=10.00, weekly=50.00, monthly=200.00)
    state = BudgetState()

    # Simulate a spend log
    log_path = Path("/tmp/budget-guard-demo.jsonl")
    config.spend_log = str(log_path)
    if log_path.exists():
        log_path.unlink()

    base_time = time.time() - (3 * 86400)  # 3 days ago

    print("=" * 60)
    print("  Budget Guard Demo: 3-Day Simulation")
    print("=" * 60)
    print()

    # Day 1: Light usage
    day1_commands = [
        ("fix typo in README", "gpt-4o", 150, 45, 0.02),
        ("refactor auth module", "gpt-4o", 2400, 890, 0.85),
        ("add test for edge case", "gpt-4o", 320, 120, 0.06),
    ]

    for i, (cmd, model, tin, tout, cost) in enumerate(day1_commands):
        ts = base_time + i * 1800  # 30 min apart
        entry = SpendEntry(timestamp=ts, cost=cost, model=model, command=cmd, tokens_in=tin, tokens_out=tout)
        throttled = state.record(entry, config)
        print(f"  [{entry.datetime_utc}] {cmd}")
        print(f"    {model:20s}  in={tin:>5d}  out={tout:>4d}  ${cost:.2f}")

    print()
    print(f"  Day 1 total: ${state.daily_spend:.2f} / ${config.daily:.2f}")
    print()

    # Day 2: Heavier usage — hits 85% threshold
    day2_commands = [
        ("implement rate limiter middleware", "gpt-4o", 5200, 1800, 1.80),
        ("rewrite database migration script", "gpt-4o", 3800, 1400, 1.35),
        ("add webhook notification handler", "gpt-4o", 4100, 1600, 1.50),
        ("optimize SQL queries in reports", "gpt-4o", 4500, 1700, 0.92),
    ]
    day2_time = base_time + 86400

    for i, (cmd, model, tin, tout, cost) in enumerate(day2_commands):
        ts = day2_time + i * 3600
        entry = SpendEntry(timestamp=ts, cost=cost, model=model, command=cmd, tokens_in=tin, tokens_out=tout)
        throttled = state.record(entry, config)

        prefix = "  ⚡" if throttled else "   "
        print(f"  {prefix} [{entry.datetime_utc}] {cmd}")
        print(f"     {model:20s}  in={tin:>5d}  out={tout:>4d}  ${cost:.2f}")
        if throttled:
            print(f"     ▶ 85% daily budget hit! Auto-throttling to {state.economy_model}")

    print()
    print(f"  Day 2 total: ${sum(e.cost for e in state.entries if e.timestamp >= day2_time):.2f} / remaining daily: ${max(0, config.daily - state.daily_spend):.2f}")

    # Day 3: Working under throttle on economy model
    day3_time = day2_time + 86400
    day3_commands = [
        ("fix regression in auth module", "gpt-4o-mini", 800, 300, 0.04),
        ("update API documentation", "gpt-4o-mini", 1200, 500, 0.06),
        ("write unit test for throttling", "gpt-4o-mini", 600, 250, 0.03),
        ("bump version in Cargo.toml", "gpt-4o-mini", 200, 80, 0.01),
        ("polish error messages", "gpt-4o-mini", 900, 350, 0.05),
        ("final review pass", "gpt-4o-mini", 1500, 600, 0.08),
    ]

    print()
    print(f"  Day 3 — User doesn't notice throttle. Codex CLI switches model silently.")
    print()

    for i, (cmd, model, tin, tout, cost) in enumerate(day3_commands):
        ts = day3_time + i * 1800
        entry = SpendEntry(timestamp=ts, cost=cost, model=model, command=cmd, tokens_in=tin, tokens_out=tout)
        state.record(entry, config)
        print(f"     [{entry.datetime_utc}] {cmd}")
        print(f"     {model:20s}  in={tin:>5d}  out={tout:>4d}  ${cost:.2f}")

    day3_spend = sum(
        e.cost for e in state.entries
        if e.timestamp >= day3_time
    )

    print()
    print(f"  Day 3 total: ${day3_spend:.2f} vs. ~$8.00 if still on gpt-4o")
    print(f"  Day 3 saved: ~${8.00 - day3_spend:.2f}")
    print()

    # Summary
    print("=" * 60)
    print("  3-Day Summary")
    print("=" * 60)
    print()
    total_day1 = sum(e.cost for e in state.entries if day2_time - 86400 <= e.timestamp < day2_time)
    total_day2 = sum(e.cost for e in state.entries if day3_time - 86400 <= e.timestamp < day3_time)
    print(f"  Day 1:  ${total_day1:.2f}  — light usage, full model")
    print(f"  Day 2:  ${total_day2:.2f}  — heavy usage, hit 85% threshold")
    print(f"  Day 3:  ${day3_spend:.2f}  — throttled, economy model")
    print(f"  ─────────────────")
    print(f"  Total:  ${state.monthly_spend:.2f}")
    print()
    print(f"  Without budget guard (all gpt-4o): ~$47.00")
    print(f"  With budget guard:          ${state.monthly_spend:.2f}")
    print(f"  Savings:                    ~${47.00 - state.monthly_spend:.2f}")
    print()

    # Clean up
    log_path.unlink(missing_ok=True)


def main() -> None:
    parser = argparse.ArgumentParser(description="Budget Guard — automatic cost-aware model throttling")
    sub = parser.add_subparsers(dest="command")

    p_status = sub.add_parser("status", help="Show current budget state")
    p_status.set_defaults(func=cmd_status)

    p_log = sub.add_parser("log", help="Show recent spend log")
    p_log.add_argument("--limit", "-n", type=int, default=10, help="Number of entries")
    p_log.set_defaults(func=cmd_log)

    p_record = sub.add_parser("record", help="Record a command's cost")
    p_record.add_argument("--cost", type=float, required=True)
    p_record.add_argument("--model", type=str, default="gpt-4o")
    p_record.add_argument("--command", type=str, default="unknown")
    p_record.add_argument("--tokens-in", type=int, default=0)
    p_record.add_argument("--tokens-out", type=int, default=0)
    p_record.set_defaults(func=cmd_record)

    p_check = sub.add_parser("check", help="Check recommended model")
    p_check.set_defaults(func=cmd_check)

    p_demo = sub.add_parser("demo", help="Run a simulated demo")
    p_demo.set_defaults(func=cmd_demo)

    args = parser.parse_args()
    if not hasattr(args, "func"):
        parser.print_help()
        sys.exit(1)

    args.func(args)


if __name__ == "__main__":
    main()
