#!/bin/bash
# Budget Guard post-command hook.
# Called by Codex CLI after each command completes.
# Records token spend to the budget tracker.
#
# Required environment variables:
#   BUDGET_GUARD_SCRIPT - path to budget_guard.py
#   CODEX_CMD_COST      - cost of the command in USD
#   CODEX_CMD_MODEL     - model used
#   CODEX_CMD_NAME      - command name/description
#   CODEX_CMD_TOKENS_IN  - input token count
#   CODEX_CMD_TOKENS_OUT - output token count

BUDGET_GUARD="${BUDGET_GUARD_SCRIPT:-$(dirname "$0")/../budget_guard.py}"

if [ -z "$CODEX_CMD_COST" ]; then
    exit 0
fi

python3 "$BUDGET_GUARD" record \
    --cost "$CODEX_CMD_COST" \
    --model "${CODEX_CMD_MODEL:-unknown}" \
    --command "${CODEX_CMD_NAME:-unknown}" \
    --tokens-in "${CODEX_CMD_TOKENS_IN:-0}" \
    --tokens-out "${CODEX_CMD_TOKENS_OUT:-0}"
