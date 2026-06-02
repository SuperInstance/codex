/// Example: Phase detection visualization.
use codex_budget_guard::{BudgetAction, BudgetConfig, BudgetGuard};
use conservation_checker::Phase;

const SPENDING_PATTERN: &[u64] = &[
    2_000,
    2_500,
    3_000,
    4_000,
    6_000,
    10_000,
    15_000,
    5_000,
    2_000,
];

fn phase_icon(phase: Phase) -> &'static str {
    match phase {
        Phase::Stable => "✅",
        Phase::PreTransition => "⚠️",
        Phase::Transitioning => "🚨",
        Phase::Resolving => "🔄",
    }
}

fn main() {
    println!("=== Budget Guard Phase Detection Demo ===\n");

    let config = BudgetConfig::builder()
        .daily(100_000)
        .warmup_records(3)
        .build();

    let mut guard = BudgetGuard::new("phase-demo", config);

    for (i, &spend) in SPENDING_PATTERN.iter().enumerate() {
        let action = guard.record(spend, "gpt-5-codex").unwrap();
        let daily_remaining = guard.checker().current_value("daily");
        let daily_phase = guard.checker().phase("daily");
        let drift = guard.checker().drift_rate("daily");

        let action_str = match &action {
            BudgetAction::Proceed(m) => format!("Proceed({m})"),
            BudgetAction::Throttle(m) => format!("Throttle→{m}"),
            BudgetAction::Halt => "HALT".into(),
        };

        println!(
            "Turn {:2}: spent {:>5} tokens | remaining: {:>6.0} | {} {:?} (drift: {:>+.0}/rec) | {}",
            i + 1,
            spend,
            daily_remaining,
            phase_icon(daily_phase),
            daily_phase,
            drift,
            action_str,
        );
    }

    println!();
    println!("--- Key takeaways ---");
    println!("Stable → Budget well within limits");
    println!("PreTransition → Spending accelerating, consider downgrade");
    println!("Transitioning → Critical, immediate action needed");
    println!("Resolving → Was critical but recovering, maintain throttle");
}
