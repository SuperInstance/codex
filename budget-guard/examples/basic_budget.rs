/// Example: Basic budget setup with daily, weekly, and monthly limits.
use codex_budget_guard::{BudgetAction, BudgetConfig, BudgetGuard};

fn main() {
    println!("=== Basic Budget Example ===\n");

    let config = BudgetConfig::builder()
        .daily(10_000)
        .weekly(50_000)
        .monthly(200_000)
        .tolerance(0.10)
        .warmup_records(3)
        .build();

    let mut guard = BudgetGuard::new("budget-demo", config);

    let requests = [
        (2000, "gpt-4.1-mini"),
        (3500, "gpt-4.1-mini"),
        (1500, "gpt-4.1-mini"),
        (5000, "gpt-4.1-mini"),
        (1000, "gpt-4.1-mini"),
    ];

    for (i, (tokens, model)) in requests.iter().enumerate() {
        let action = guard.record(*tokens, model).unwrap();

        let action_str = match &action {
            BudgetAction::Proceed(m) => format!("✅ Proceed({m})"),
            BudgetAction::Throttle(m) => format!("⚠️ Throttle→{m}"),
            BudgetAction::Halt => "🛑 HALT".into(),
        };

        println!(
            "Request {}: {:>5} tokens (model: {}) | {}",
            i + 1,
            tokens,
            model,
            action_str,
        );
    }

    println!("\n=== Summary ===");
    println!("  Cumulative: {:.0} tokens", guard.cumulative_total());
    println!("  Records: {}", guard.record_count());
    println!("  Throttle level: {}", guard.throttle_level());
}
