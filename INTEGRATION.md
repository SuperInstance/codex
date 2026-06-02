# Budget Guard Integration

> Token spending limits for OpenAI Codex CLI.

## What was added

A standalone Rust crate at [`budget-guard/`](./budget-guard/) that enforces daily, weekly, and monthly token budgets. It runs as a wrapper around Codex CLI and intercepts the request/response loop to:

1. **Record token usage** after each API response completes
2. **Detect spending acceleration** using phase analysis (60% → 85% → exhausted)
3. **Auto-downgrade models** when budgets are tight (e.g. GPT-5-codex → GPT-4.1 → GPT-4.1-mini)
4. **Halt** when all budgets are exhausted
5. **Log spending** as JSON for auditing

## Why

Codex CLI (87K+ stars) is the best terminal coding agent. But every `codex 'fix this bug'` costs tokens — and tokens cost money. The original codebase tracks tokens per call but has **no budget enforcement**. This crate fills that gap.

Our fork provides a drop-in solution: a configurable guard that sits between Codex and the API, watching spending and preventing runaway bills.

## How it works

The `BudgetGuard` uses `conservation-checker` (from crates.io) which implements one-sided conservation laws. Instead of asking "did we exceed the limit?", it tracks *remaining* tokens as a conserved quantity that must not decrease past zero. This gives early warning (PreTransition phase at ~60% consumption) and critical alerts (Transitioning at ~85%).

```
┌──────────────┐     ┌──────────────┐     ┌──────────────┐
│  Codex CLI   │────▶│ BudgetGuard  │────▶│  API Call    │
│  (completes) │     │  record()    │     │  (next turn) │
└──────────────┘     └──────┬───────┘     └──────┬───────┘
                            │                    │
                     ┌──────▼───────┐            │
                     │ recommend_   │◀───────────┘
                     │ action()     │
                     └──────┬───────┘
                            │
                     ┌──────▼───────┐
                     │  Proceed?    │
                     │  Throttle?   │
                     │  Halt?       │
                     └──────────────┘
```

## Configuration

Configure via `.budget.toml`:

```toml
[daily]
limit = 500000         # 500K tokens/day

[weekly]
limit = 2500000        # 2.5M tokens/week

[monthly]
limit = 10000000       # 10M tokens/month

[tolerance]
fraction = 0.05        # allow 5% overshoot

[throttle]
ladder = ["gpt-5-codex", "gpt-4.1", "gpt-4.1-mini", "gpt-4.1-nano"]
```

## Usage

```bash
# Build the budget guard
cd budget-guard && cargo build --release

# Run with defaults (500K daily / 2.5M weekly / 10M monthly)
./target/release/codex-budget-guard --budget .budget.toml -- codex
```

## Phase thresholds

| Consumption | Phase | Action |
|------------|-------|--------|
| 0-60% | Stable | Proceed with current model |
| 60-85% | PreTransition | Warning: spending accelerating |
| 85-100% | Transitioning | Auto-throttle to cheaper model |
| 100%+ | Exhausted | Halt — block further requests |

## Audit logging

Every record is logged to `budget-audit.json` with full period snapshots:

```json
{
  "session_id": "codex-session-42",
  "timestamp_ms": 1748815200000,
  "periods": {
    "daily": {
      "limit": 500000.0,
      "consumed": 234500.0,
      "remaining": 265500.0,
      "violated": false,
      "phase": "Stable",
      "drift_rate": -15000.0
    }
  },
  "cumulative_total": 234500.0,
  "throttle_level": 0,
  "active_model": "gpt-5-codex"
}
```

## File layout

```
codex/
├── budget-guard/           # <-- Added: Rust crate
│   ├── Cargo.toml
│   ├── src/lib.rs          # BudgetGuard + BudgetConfig + BudgetAction
│   ├── examples/
│   │   ├── basic_budget.rs
│   │   ├── integrated.rs
│   │   ├── phase_demo.rs
│   │   └── team_usage.rs
├── INTEGRATION.md          # <-- Added: this file
└── README.md               # <-- Updated: added enhancement notice
```
