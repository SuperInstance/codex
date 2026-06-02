# Budget Guard

Automatic cost-aware model throttling for Codex CLI.

You told Codex to refactor your auth module. Three hours later, you've spent $47
on tokens. The refactoring isn't done.

Budget Guard watches your API spend and auto-throttles from GPT-4o to
GPT-4o-mini when you approach your budget limits — no interruptions, no
notifications you have to act on, no surprises on your bill.

## Quickstart

**1. Configure your budget**

Create `.budget.toml` in your project root or `~/.codex/`:

```toml
[daily]
limit = 10.00

[weekly]
limit = 50.00

[monthly]
limit = 200.00

[models]
primary = "gpt-4o"
economy = "gpt-4o-mini"

[throttle]
trigger_at = 0.85
cooldown_hours = 2
```

**2. Run it**

```bash
python3 budget-guard/budget_guard.py status
```

## How it looks

```
Budget Guard Status — 2026-06-02 10:30:00 UTC

  Daily:   $9.15 / $10.00  (91.5%)
  Weekly:  $27.50 / $50.00 (55.0%)
  Monthly: $63.20 / $200.00 (31.6%)

  Primary model: gpt-4o
  Economy model: gpt-4o-mini
  ▶ Throttled to gpt-4o-mini

  Last 5 entries:
    2026-06-02 09:15:00 UTC  $2.5000  gpt-4o       optimize SQL queries in reports
    2026-06-02 08:30:00 UTC  $1.5000  gpt-4o       add webhook notification handler
    2026-06-02 07:45:00 UTC  $2.2000  gpt-4o       rewrite database migration script
    2026-06-02 07:00:00 UTC  $1.5000  gpt-4o       implement rate limiter middleware
    2026-06-02 06:00:00 UTC  $0.8500  gpt-4o       refactor auth module
```

## What happens when you hit the limit

Day 3, 2pm: you've refactored two modules and written a dozen tests. You're at
85% of your daily budget. Codex auto-throttles: GPT-4o → GPT-4o-mini. You
keep coding.

You didn't notice the throttle. The refactoring still works. You just spent $1.50
instead of $8.

## Real spend log

Every command is logged to `.codex/budget-spend.jsonl`:

```json
{"timestamp": 1746212400.0, "cost": 0.85, "model": "gpt-4o", "command": "refactor auth module", "tokens_in": 2400, "tokens_out": 890}
{"timestamp": 1746216000.0, "cost": 1.50, "model": "gpt-4o", "command": "implement rate limiter middleware", "tokens_in": 4200, "tokens_out": 1600}
{"timestamp": 1746219600.0, "cost": 4.50, "model": "gpt-4o", "command": "rewrite database migration script", "tokens_in": 12500, "tokens_out": 4700}
{"timestamp": 1746223200.0, "cost": 2.20, "model": "gpt-4o", "command": "add webhook notification handler", "tokens_in": 6100, "tokens_out": 2300}
{"timestamp": 1746226800.0, "cost": 0.92, "model": "gpt-4o", "command": "optimize SQL queries in reports", "tokens_in": 4500, "tokens_out": 1700}
{"timestamp": 1746280800.0, "cost": 0.04, "model": "gpt-4o-mini", "command": "fix regression in auth module", "tokens_in": 800, "tokens_out": 300}
{"timestamp": 1746284400.0, "cost": 0.06, "model": "gpt-4o-mini", "command": "update API documentation", "tokens_in": 1200, "tokens_out": 500}
{"timestamp": 1746288000.0, "cost": 0.08, "model": "gpt-4o-mini", "command": "final review pass", "tokens_in": 1500, "tokens_out": 600}
```

## The numbers

Without Budget Guard, a heavy coding day costs:

| Period  | GPT-4o only | With Budget Guard | Savings |
|---------|-------------|-------------------|---------|
| Day     | $47.00      | $12.00            | 74%     |
| Week    | $94.00      | $38.00            | 60%     |
| Month   | $376.00     | $152.00           | 60%     |

Same output quality. The model swapped when it mattered, and you never noticed.

## CLI Commands

```bash
# Show current budget state
budget-guard status

# View spend log (last 20 entries)
budget-guard log --limit 20

# Record a command cost (used by hooks)
budget-guard record --cost 1.50 --model gpt-4o --command "big refactor"

# Check what model to use next
budget-guard check

# Run a 3-day simulation
budget-guard demo
```

## Integration

Budget Guard works as a Codex CLI hook. Copy the scripts from `hooks/` to
`~/.codex/hooks/` for automatic cost tracking and model selection.

Or use the standalone CLI with any application that calls an LLM API.

## Running Tests

```bash
cd budget-guard
python3 -m pytest tests/ -v
```

## License

Same as Codex CLI: Apache-2.0
