# Budget Guard Hooks

These hooks integrate Budget Guard with Codex CLI. When installed, they
automatically record token spend after each command and adjust the model
selection based on your budget.

## Installation

```bash
# Copy hooks into your codex home directory
cp budget-guard/hooks/*.sh ~/.codex/hooks/
```

## How it works

1. **`post-command` hook**: After each CLI command, reads the token usage from
   the response and records it to `.codex/budget-spend.jsonl`.
2. **`model-select` hook**: Before starting a new turn, checks the budget
   and recommends either the primary or economy model.

## Manual Usage

You can also use the CLI directly:

```bash
# Check current status
python3 budget-guard/budget_guard.py status

# See recent spend
python3 budget-guard/budget_guard.py log

# Check recommended model (returns JSON)
python3 budget-guard/budget_guard.py check
```
