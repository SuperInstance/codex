/// Example: Team usage scenario with shared budget tracking.
use codex_budget_guard::{BudgetAction, BudgetConfig, BudgetGuard};

fn main() {
    println!("=== Team Budget Scenario: $5000/month ===\n");

    let config = BudgetConfig::builder()
        .daily(3_000_000)
        .weekly(15_000_000)
        .monthly(60_000_000)
        .tolerance(0.05)
        .throttle_ladder(vec![
            "gpt-5-codex".into(),
            "gpt-4.1".into(),
            "gpt-4.1-mini".into(),
        ])
        .warmup_records(10)
        .build();

    let mut guard = BudgetGuard::new("team-alpha", config);

    struct Turn {
        user: &'static str,
        tokens: u64,
        model: &'static str,
    }

    let turns = vec![
        Turn { user: "alice", tokens: 250_000, model: "gpt-5-codex" },
        Turn { user: "bob",   tokens: 180_000, model: "gpt-5-codex" },
        Turn { user: "carol", tokens: 320_000, model: "gpt-5-codex" },
        Turn { user: "alice", tokens: 500_000, model: "gpt-5-codex" },
        Turn { user: "bob",   tokens: 800_000, model: "gpt-5-codex" },
        Turn { user: "carol", tokens: 950_000, model: "gpt-5-codex" },
        Turn { user: "alice", tokens: 200_000, model: "gpt-5-codex" },
        Turn { user: "bob",   tokens: 100_000, model: "gpt-4.1" },
    ];

    for (i, turn) in turns.iter().enumerate() {
        let action = guard.record(turn.tokens, turn.model).unwrap();

        match &action {
            BudgetAction::Proceed(m) => {
                println!("Turn {:2} [{}]: {:>7} tokens | ✅ Proceed({m})", i+1, turn.user, turn.tokens);
            }
            BudgetAction::Throttle(m) => {
                println!("Turn {:2} [{}]: {:>7} tokens | ⚠️ Throttle→{m}", i+1, turn.user, turn.tokens);
            }
            BudgetAction::Halt => {
                println!("Turn {:2} [{}]: {:>7} tokens | 🛑 HALT", i+1, turn.user, turn.tokens);
            }
        }
    }

    println!("\n=== Team Summary ===");
    println!("  Total tokens: {:.0}", guard.cumulative_total());
    println!("  Throttle level: {}", guard.throttle_level());
    println!("  Active model: {}", guard.active_model());
}
