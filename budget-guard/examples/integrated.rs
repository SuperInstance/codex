/// Example: Full integration of BudgetGuard with Codex CLI workflow simulation.
use codex_budget_guard::{BudgetAction, BudgetConfig, BudgetGuard};

fn cost_per_call(model: &str) -> u64 {
    match model {
        "gpt-5-codex" => 12_000,
        "gpt-4.1" => 8_000,
        "gpt-4.1-mini" => 4_000,
        "gpt-4.1-nano" => 1_500,
        _ => 10_000,
    }
}

fn run_codex_turn(guard: &mut BudgetGuard, model: &str) {
    let tokens = cost_per_call(model);
    match guard.record(tokens, model).unwrap() {
        BudgetAction::Proceed(m) => {
            println!("  ✅ Proceed with {m} ({tokens} tokens used, total={})", guard.cumulative_total());
        }
        BudgetAction::Throttle(new_model) => {
            println!("  ⚠️  Throttle: {model} → {new_model}");
        }
        BudgetAction::Halt => {
            println!("  🛑 HALT — no budget remaining");
        }
    }
}

fn main() {
    println!("=== Codex Budget Guard: Full Integration Demo ===\n");

    let config = BudgetConfig::builder()
        .daily(500_000)
        .weekly(2_500_000)
        .monthly(10_000_000)
        .tolerance(0.05)
        .throttle_ladder(vec![
            "gpt-5-codex".into(),
            "gpt-4.1".into(),
            "gpt-4.1-mini".into(),
            "gpt-4.1-nano".into(),
        ])
        .warmup_records(3)
        .build();

    let mut guard = BudgetGuard::new("codex-session-42", config);

    for i in 1..=10 {
        let model = if guard.throttle_level() == 0 {
            "gpt-5-codex"
        } else if guard.throttle_level() <= 1 {
            "gpt-4.1"
        } else if guard.throttle_level() <= 2 {
            "gpt-4.1-mini"
        } else {
            "gpt-4.1-nano"
        };

        println!("\nTurn {i} (throttle_level={}, model={model}):", guard.throttle_level());
        run_codex_turn(&mut guard, model);

        if i == 6 {
            let snap = guard.snapshot_json().unwrap();
            println!("\n  📸 Audit snapshot:\n{}", &snap[..snap.len().min(400)]);
        }
    }

    println!("\n=== Final Audit ===");
    let snapshot = guard.snapshot_json().unwrap();
    let sv: serde_json::Value = serde_json::from_str(&snapshot).unwrap();
    println!("  Cumulative tokens: {}", sv["cumulative_total"]);
    println!("  Records: {}", guard.record_count());
}
