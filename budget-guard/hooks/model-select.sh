#!/bin/bash
# Budget Guard model-select hook.
# Called by Codex CLI before starting a new turn.
# Outputs the recommended model based on budget state.
#
# Output: prints the model name (e.g., "gpt-4o" or "gpt-4o-mini")
# to stdout. The CLI uses this as the model for the next turn.

BUDGET_GUARD="${BUDGET_GUARD_SCRIPT:-$(dirname "$0")/../budget_guard.py}"

python3 -c "
import json, sys
sys.path.insert(0, '$(dirname "$0")/..')
from budget_guard import BudgetConfig, BudgetState

config = BudgetConfig.load()
state = BudgetState()
state.load(config)
print(state.recommended_model)
"
