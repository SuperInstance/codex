"""Tests for budget-guard/budget_guard.py - Budget Guard."""

import json
import os
import time
from pathlib import Path
from unittest import mock

import pytest

# Ensure budget_guard is importable
import sys
sys.path.insert(0, str(Path(__file__).resolve().parent.parent))

from budget_guard import BudgetConfig, BudgetState, SpendEntry


# ═══════════════════════════════════════════════════════════════════
# BudgetConfig Tests
# ═══════════════════════════════════════════════════════════════════

class TestBudgetConfig:
    def test_defaults(self):
        c = BudgetConfig()
        assert c.daily == 10.00
        assert c.weekly == 50.00
        assert c.monthly == 200.00
        assert c.primary_model == "gpt-4o"
        assert c.economy_model == "gpt-4o-mini"
        assert c.trigger_at == 0.85
        assert c.cooldown_hours == 2.0

    def test_load_from_toml(self, tmp_path):
        toml_path = tmp_path / ".budget.toml"
        toml_path.write_text("""
[daily]
limit = 5.00

[weekly]
limit = 25.00

[monthly]
limit = 100.00

[models]
primary = "gpt-4o"
economy = "claude-3-haiku"

[throttle]
trigger_at = 0.75
cooldown_hours = 1.0

[logging]
spend_log = "/tmp/test-spend.jsonl"
verbose = true
""")
        c = BudgetConfig.load(toml_path)
        assert c.daily == 5.00
        assert c.weekly == 25.00
        assert c.monthly == 100.00
        assert c.economy_model == "claude-3-haiku"
        assert c.trigger_at == 0.75
        assert c.cooldown_hours == 1.0
        assert c.spend_log == "/tmp/test-spend.jsonl"

    def test_load_partial_toml(self, tmp_path):
        """Partial config should keep defaults for missing fields."""
        toml_path = tmp_path / ".budget.toml"
        toml_path.write_text("""
[daily]
limit = 3.00

[models]
economy = "gpt-4o-mini"
""")
        c = BudgetConfig.load(toml_path)
        assert c.daily == 3.00
        assert c.weekly == 50.00  # default
        assert c.monthly == 200.00  # default
        assert c.primary_model == "gpt-4o"  # default
        assert c.economy_model == "gpt-4o-mini"
        assert c.trigger_at == 0.85  # default

    def test_load_from_nonexistent_path_returns_defaults(self):
        c = BudgetConfig.load(Path("/nonexistent/.budget.toml"))
        assert c.daily == 10.00  # defaults

    def test_load_with_empty_dict_returns_defaults(self, tmp_path):
        toml_path = tmp_path / ".budget.toml"
        toml_path.write_text("")
        c = BudgetConfig.load(toml_path)
        assert c.daily == 10.00

    def test_find_toml_searches_upward(self, tmp_path):
        """Should find .budget.toml in a parent directory."""
        deep = tmp_path / "a" / "b" / "c"
        deep.mkdir(parents=True)
        toml = tmp_path / ".budget.toml"
        toml.write_text('[daily]\nlimit = 7.00\n')

        with mock.patch("pathlib.Path.cwd", return_value=deep):
            c = BudgetConfig()
            found = c._find_toml()
            assert found is not None
            assert found == toml

    def test_find_toml_home_fallback(self):
        """Should fall back to ~/.codex/.budget.toml if none in cwd tree."""
        home_budget = Path.home() / ".codex" / ".budget.toml"
        if home_budget.exists():
            c = BudgetConfig()
            found = c._find_toml()
            assert found == home_budget
        else:
            c = BudgetConfig()
            with mock.patch("pathlib.Path.exists", return_value=False):
                found = c._find_toml()
                assert found is None


# ═══════════════════════════════════════════════════════════════════
# SpendEntry Tests
# ═══════════════════════════════════════════════════════════════════

class TestSpendEntry:
    def test_create_entry(self):
        now = time.time()
        e = SpendEntry(
            timestamp=now,
            cost=0.85,
            model="gpt-4o",
            command="refactor auth module",
            tokens_in=2400,
            tokens_out=890,
        )
        assert e.cost == 0.85
        assert e.model == "gpt-4o"
        assert "UTC" in e.datetime_utc

    def test_to_from_json_roundtrip(self):
        now = time.time()
        e = SpendEntry(
            timestamp=now,
            cost=1.23,
            model="gpt-4o-mini",
            command="fix bug in parser",
            tokens_in=500,
            tokens_out=200,
        )
        data = e.to_json()
        e2 = SpendEntry.from_json(data)
        assert e.timestamp == e2.timestamp
        assert e.cost == e2.cost
        assert e.model == e2.model
        assert e.command == e2.command
        assert e.tokens_in == e2.tokens_in
        assert e.tokens_out == e2.tokens_out


# ═══════════════════════════════════════════════════════════════════
# BudgetState Tests
# ═══════════════════════════════════════════════════════════════════

class TestBudgetState:
    def test_initial_state(self):
        s = BudgetState()
        assert s.daily_spend == 0.0
        assert s.weekly_spend == 0.0
        assert s.monthly_spend == 0.0
        assert not s.is_throttled
        assert s.recommended_model == "gpt-4o"

    def test_record_entry_under_threshold(self, tmp_path):
        config = BudgetConfig()
        log_path = tmp_path / "spend.jsonl"
        config.spend_log = str(log_path)

        state = BudgetState()
        state.load(config)

        entry = SpendEntry(
            timestamp=time.time(),
            cost=0.05,
            model="gpt-4o",
            command="small fix",
        )
        throttled = state.record(entry, config)

        assert not throttled
        assert state.daily_spend == 0.05
        assert state.recommended_model == "gpt-4o"

    def test_record_entry_triggers_throttle(self, tmp_path):
        config = BudgetConfig(daily=10.00, trigger_at=0.85)
        log_path = tmp_path / "spend.jsonl"
        config.spend_log = str(log_path)

        state = BudgetState()
        state.load(config)

        entry = SpendEntry(
            timestamp=time.time(),
            cost=9.00,
            model="gpt-4o",
            command="big refactor",
            tokens_in=25000,
            tokens_out=8000,
        )
        throttled = state.record(entry, config)

        assert throttled
        assert state.is_throttled
        assert state.recommended_model == "gpt-4o-mini"
        assert state.daily_spend == 9.00
        assert state.daily_pct == 0.9

    def test_throttle_persists_across_entries(self, tmp_path):
        config = BudgetConfig(daily=10.00, trigger_at=0.85, cooldown_hours=24)
        log_path = tmp_path / "spend.jsonl"
        config.spend_log = str(log_path)

        state = BudgetState()
        state.load(config)

        state.record(SpendEntry(timestamp=time.time(), cost=9.00, model="gpt-4o", command="big task"), config)
        state.record(SpendEntry(timestamp=time.time(), cost=0.10, model="gpt-4o-mini", command="small task"), config)

        assert state.is_throttled
        assert state.recommended_model == "gpt-4o-mini"

    def test_throttle_weekly_window(self, tmp_path):
        config = BudgetConfig(weekly=50.00, daily=100.00, trigger_at=0.85)
        log_path = tmp_path / "spend.jsonl"
        config.spend_log = str(log_path)

        state = BudgetState()
        state.load(config)

        for i in range(5):
            ts = time.time() - (i * 86400)
            state.record(
                SpendEntry(timestamp=ts, cost=9.00, model="gpt-4o", command=f"task day {i}"),
                config,
            )

        assert state.daily_pct < 0.85
        assert state.weekly_pct >= 0.85
        assert state.is_throttled
        assert state.recommended_model == "gpt-4o-mini"

    def test_weekly_spend_recovery(self, tmp_path):
        """When weekly spend drops back below threshold, should un-throttle."""
        config = BudgetConfig(weekly=50.00, daily=100.00, trigger_at=0.85, cooldown_hours=0)
        log_path = tmp_path / "spend.jsonl"
        config.spend_log = str(log_path)

        state = BudgetState()
        state.load(config)

        for i in range(5):
            ts = time.time() - (i * 86400)
            state.record(
                SpendEntry(timestamp=ts, cost=9.00, model="gpt-4o", command=f"task {i}"),
                config,
            )

        assert state.is_throttled
        assert state.recommended_model == "gpt-4o-mini"

    def test_percentage_calculation(self, tmp_path):
        config = BudgetConfig(daily=10.00, weekly=50.00, monthly=200.00)
        log_path = tmp_path / "spend.jsonl"
        config.spend_log = str(log_path)

        state = BudgetState()
        state.load(config)

        state.record(SpendEntry(timestamp=time.time(), cost=2.50, model="gpt-4o", command="test"), config)

        assert state.daily_pct == 0.25
        assert state.weekly_pct == 0.05
        assert state.monthly_pct == pytest.approx(0.0125, abs=0.001)

    def test_zero_limit_does_not_divide_by_zero(self):
        config = BudgetConfig(daily=0, weekly=0, monthly=0)
        s = BudgetState()
        s.load(config)
        s.daily_spend = 5.00
        assert s.daily_pct == 0.0
        assert s.weekly_pct == 0.0
        assert s.monthly_pct == 0.0

    def test_status_report_format(self, tmp_path):
        config = BudgetConfig(daily=10.00)
        log_path = tmp_path / "spend.jsonl"
        config.spend_log = str(log_path)

        state = BudgetState()
        state.load(config)
        state.record(SpendEntry(timestamp=time.time(), cost=3.00, model="gpt-4o", command="test"), config)

        report = state.status_report()
        assert "Budget Guard Status" in report
        assert "$3.00" in report
        assert "gpt-4o" in report

    def test_empty_log_file(self, tmp_path):
        config = BudgetConfig()
        log_path = tmp_path / "empty.jsonl"
        log_path.write_text("")
        config.spend_log = str(log_path)

        state = BudgetState()
        state.load(config)
        assert state.daily_spend == 0.0
        assert len(state.entries) == 0

    def test_corrupted_log_line_skipped(self, tmp_path):
        config = BudgetConfig()
        log_path = tmp_path / "corrupt.jsonl"
        ts = time.time() - 100  # 100 seconds ago, within daily window
        log_path.write_text(
            '{"timestamp": %.1f, "cost": 1.0, "model": "gpt-4o", "command": "ok"}\n'
            'not-json\n'
            '{"timestamp": %.1f, "cost": 2.0, "model": "gpt-4o-mini", "command": "also ok"}\n'
            % (ts, ts)
        )
        config.spend_log = str(log_path)
    
        state = BudgetState()
        state.load(config)
        assert len(state.entries) == 2
        assert state.daily_spend == pytest.approx(3.0, abs=0.01)
        assert state.monthly_spend == pytest.approx(3.0, abs=0.01)

    def test_multiple_sessions_same_day(self, tmp_path):
        config = BudgetConfig(daily=10.00, trigger_at=0.50)
        log_path = tmp_path / "spend.jsonl"
        config.spend_log = str(log_path)

        state = BudgetState()
        state.load(config)

        state.record(SpendEntry(timestamp=time.time(), cost=1.00, model="gpt-4o", command="session1 task1"), config)
        state.record(SpendEntry(timestamp=time.time(), cost=2.00, model="gpt-4o", command="session1 task2"), config)

        # New session reload
        state2 = BudgetState()
        state2.load(config)
        assert len(state2.entries) == 2
        assert state2.daily_spend == 3.00  # 30% - under 50% trigger
        assert state2.recommended_model == "gpt-4o"  # not throttled

        # Cross 50%
        state2.record(SpendEntry(cost=2.50, model="gpt-4o", command="session2 task", timestamp=time.time()), config)
        assert state2.daily_spend == 5.50  # 55% > 50% trigger
        assert state2.is_throttled


# ═══════════════════════════════════════════════════════════════════
# Integration / Scenario Tests
# ═══════════════════════════════════════════════════════════════════

class TestScenario:
    def test_budget_threshold_crossing(self, tmp_path):
        """Cross daily threshold, verify throttle engages."""
        config = BudgetConfig(daily=10.00, trigger_at=0.85)
        log_path = tmp_path / "threshold.jsonl"
        config.spend_log = str(log_path)

        state = BudgetState()
        state.load(config)

        # Stay under threshold
        state.record(SpendEntry(timestamp=time.time(), cost=8.40, model="gpt-4o", command="big task"), config)
        assert not state.is_throttled  # 84% < 85%

        # Cross threshold
        state.record(SpendEntry(timestamp=time.time(), cost=0.20, model="gpt-4o", command="extra task"), config)
        assert state.is_throttled  # 86% > 85%

    def test_budget_recommended_model(self, tmp_path):
        """recommended_model should change when throttled."""
        config = BudgetConfig(daily=10.00, trigger_at=0.50)
        log_path = tmp_path / "model.jsonl"
        config.spend_log = str(log_path)

        state = BudgetState()
        state.load(config)

        assert state.recommended_model == "gpt-4o"

        state.record(SpendEntry(timestamp=time.time(), cost=6.00, model="gpt-4o", command="big task"), config)

        assert state.is_throttled
        assert state.recommended_model == "gpt-4o-mini"

    def test_cost_savings_ratio(self):
        """Economy model should cost significantly less than primary."""
        # Same workload on gpt-4o
        gpt4o_entries = [
            SpendEntry(timestamp=time.time(), cost=2.50, model="gpt-4o", command="task 1", tokens_in=7000, tokens_out=2600),
            SpendEntry(timestamp=time.time(), cost=3.00, model="gpt-4o", command="task 2", tokens_in=8400, tokens_out=3100),
            SpendEntry(timestamp=time.time(), cost=1.50, model="gpt-4o", command="task 3", tokens_in=4200, tokens_out=1500),
        ]

        # Same workload on gpt-4o-mini
        mini_entries = [
            SpendEntry(timestamp=time.time(), cost=0.14, model="gpt-4o-mini", command="task 1", tokens_in=7000, tokens_out=2600),
            SpendEntry(timestamp=time.time(), cost=0.17, model="gpt-4o-mini", command="task 2", tokens_in=8400, tokens_out=3100),
            SpendEntry(timestamp=time.time(), cost=0.08, model="gpt-4o-mini", command="task 3", tokens_in=4200, tokens_out=1500),
        ]

        total_gpt4o = sum(e.cost for e in gpt4o_entries)
        total_mini = sum(e.cost for e in mini_entries)

        assert total_gpt4o == pytest.approx(7.00, abs=0.01)
        assert total_mini == pytest.approx(0.39, abs=0.01)
        assert total_gpt4o / total_mini > 15

    def test_mixed_model_tracking(self, tmp_path):
        """Track spend across different models."""
        config = BudgetConfig(daily=50.00, trigger_at=0.85)
        log_path = tmp_path / "mixed.jsonl"
        config.spend_log = str(log_path)
    
        state = BudgetState()
        state.load(config)
    
        state.record(SpendEntry(timestamp=time.time(), cost=5.00, model="gpt-4o", command="planning"), config)
        state.record(SpendEntry(timestamp=time.time(), cost=0.12, model="gpt-4o-mini", command="simple edit"), config)
        state.record(SpendEntry(timestamp=time.time(), cost=8.00, model="gpt-4o", command="complex refactor"), config)
    
        assert state.daily_spend == pytest.approx(13.12, abs=0.001)
        assert not state.is_throttled  # 26.24% < 85%


if __name__ == "__main__":
    pytest.main([__file__, "-v"])
