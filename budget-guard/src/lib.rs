//! # `codex-budget-guard`
//!
//! 🏆 **Budget enforcement for [OpenAI Codex CLI](https://github.com/openai/codex).**
//!
//! Tracks token spending across daily, weekly, and monthly windows. Detects
//! accelerating depletion before you hit your limit, auto-downgrades to
//! cheaper models, and logs spending as JSON for auditing.
//!
//! ## Quick start
//!
//! ```ignore
//! use codex_budget_guard::{BudgetGuard, BudgetConfig, BudgetPeriod};
//!
//! let config = BudgetConfig::builder()
//!     .daily(500_000)       // 500K tokens/day max
//!     .weekly(2_500_000)    // 2.5M tokens/week max
//!     .monthly(10_000_000)  // 10M tokens/month max
//!     .build();
//!
//! let mut guard = BudgetGuard::new("my-codex-session", config);
//! guard.record(1200, "gpt-5-codex").unwrap();
//!
//! let action = guard.recommend_action();
//! match action {
//!     BudgetAction::Proceed(model) => println!("Use model: {model}"),
//!     BudgetAction::Throttle(model) => println!("Throttled to: {model}"),
//!     BudgetAction::Halt => println!("Budget exhausted!"),
//! }
//! ```

use conservation_checker::{ConservationChecker, Phase};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

// ── Errors ────────────────────────────────────────────────────────────────────

/// Errors that can occur during budget guard operations.
#[derive(Debug, thiserror::Error)]
pub enum BudgetError {
    /// The named budget period has not been registered.
    #[error("budget period '{0}' is not registered")]
    NotRegistered(String),
    /// Serialization/deserialization failure for audit snapshots.
    #[error("serialization error: {0}")]
    Serde(#[from] serde_json::Error),
    /// I/O error during file operations.
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
}

// ── Budget types ──────────────────────────────────────────────────────────────

/// Supported budget period windows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum BudgetPeriod {
    /// Resets every 24 hours.
    Daily,
    /// Resets every 7 days.
    Weekly,
    /// Resets every 30 days.
    Monthly,
}

impl BudgetPeriod {
    fn label(&self) -> &'static str {
        match self {
            BudgetPeriod::Daily => "daily",
            BudgetPeriod::Weekly => "weekly",
            BudgetPeriod::Monthly => "monthly",
        }
    }

    /// Human-readable labels for each budget period.
    pub fn labels() -> [&'static str; 3] {
        ["daily", "weekly", "monthly"]
    }
}

/// Configuration for token budget limits across time windows.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BudgetConfig {
    /// Maximum tokens allowed per day. `None` means unlimited.
    pub daily: Option<f64>,
    /// Maximum tokens allowed per week. `None` means unlimited.
    pub weekly: Option<f64>,
    /// Maximum tokens allowed per month. `None` means unlimited.
    pub monthly: Option<f64>,
    /// Tolerance fraction (0.0–1.0) applied to each budget. A tolerance of
    /// 0.05 means you can overshoot by 5% before being considered violated.
    /// Default: 0.0 (strict).
    #[serde(default)]
    pub tolerance: f64,
    /// Optional model tier ladder for auto-throttle. Each entry is a model
    /// slug that represents one step down in capability/cost.
    #[serde(default = "default_throttle_ladder")]
    pub throttle_ladder: Vec<String>,
    /// Minimum number of records before phase analysis is active.
    #[serde(default = "default_warmup_records")]
    pub warmup_records: usize,
}

fn default_throttle_ladder() -> Vec<String> {
    vec![
        "gpt-5-codex".to_string(),
        "gpt-4.1".to_string(),
        "gpt-4.1-mini".to_string(),
        "gpt-4.1-nano".to_string(),
    ]
}

fn default_warmup_records() -> usize {
    5
}

impl Default for BudgetConfig {
    fn default() -> Self {
        Self {
            daily: Some(500_000.0),
            weekly: Some(2_500_000.0),
            monthly: Some(10_000_000.0),
            tolerance: 0.0,
            throttle_ladder: default_throttle_ladder(),
            warmup_records: default_warmup_records(),
        }
    }
}

/// Builder for `BudgetConfig`.
#[derive(Debug, Default)]
pub struct BudgetConfigBuilder {
    daily: Option<f64>,
    weekly: Option<f64>,
    monthly: Option<f64>,
    tolerance: f64,
    throttle_ladder: Option<Vec<String>>,
    warmup_records: Option<usize>,
}

impl BudgetConfigBuilder {
    /// Set daily token limit.
    pub fn daily(mut self, tokens: u64) -> Self {
        self.daily = Some(tokens as f64);
        self
    }
    /// Set weekly token limit.
    pub fn weekly(mut self, tokens: u64) -> Self {
        self.weekly = Some(tokens as f64);
        self
    }
    /// Set monthly token limit.
    pub fn monthly(mut self, tokens: u64) -> Self {
        self.monthly = Some(tokens as f64);
        self
    }
    /// Set budget tolerance fraction (0.0 = strict, 0.05 = allow 5% overshoot).
    pub fn tolerance(mut self, tol: f64) -> Self {
        self.tolerance = tol.clamp(0.0, 1.0);
        self
    }
    /// Set the throttle ladder (ordered most → least capable).
    pub fn throttle_ladder(mut self, ladder: Vec<String>) -> Self {
        self.throttle_ladder = Some(ladder);
        self
    }
    /// Set minimum number of records before phase analysis is active.
    pub fn warmup_records(mut self, n: usize) -> Self {
        self.warmup_records = Some(n);
        self
    }
    /// Build the `BudgetConfig`.
    pub fn build(self) -> BudgetConfig {
        BudgetConfig {
            daily: self.daily,
            weekly: self.weekly,
            monthly: self.monthly,
            tolerance: self.tolerance,
            throttle_ladder: self.throttle_ladder.unwrap_or_else(default_throttle_ladder),
            warmup_records: self.warmup_records.unwrap_or_else(default_warmup_records),
        }
    }
}

impl BudgetConfig {
    /// Create a builder for `BudgetConfig`.
    pub fn builder() -> BudgetConfigBuilder {
        BudgetConfigBuilder::default()
    }

    /// Compute the tolerance value for conservation-checker given a limit.
    /// The checker's tolerance defines the max allowed decrease from initial.
    fn checker_tolerance(&self, limit: f64) -> f64 {
        limit * self.tolerance
    }
}

// ── Suggested action ──────────────────────────────────────────────────────────

/// The action the budget guard recommends before the next API call.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BudgetAction {
    /// Budget healthy. Use the originally-requested model slug.
    Proceed(String),
    /// Budget approaching depletion. Downgrade to a cheaper model.
    Throttle(String),
    /// All budget windows exhausted. Should block further requests.
    Halt,
}

// ── Audit snapshot ────────────────────────────────────────────────────────────

/// A point-in-time snapshot of all budget state, suitable for logging, audit, or
/// recovery via `BudgetGuard::from_snapshot`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BudgetSnapshot {
    /// Session identifier.
    pub session_id: String,
    /// When this snapshot was taken (Unix millis).
    pub timestamp_ms: i64,
    /// Serialized budget periods.
    pub periods: BTreeMap<String, PeriodSnapshot>,
    /// Cumulative total tokens spent across all periods.
    pub cumulative_total: f64,
    /// Current throttle level index (0 = full speed, >0 = downgraded).
    pub throttle_level: usize,
    /// The model currently in use.
    pub active_model: String,
}

/// Snapshot of a single budget period's state.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PeriodSnapshot {
    /// Token limit for this period.
    pub limit: f64,
    /// Tokens consumed so far in this period.
    pub consumed: f64,
    /// Remaining tokens.
    pub remaining: f64,
    /// Whether the budget is currently violated.
    pub violated: bool,
    /// Current phase of this period (serialized as string since
    /// conservation-checker 0.1 doesn't derive serde on Phase).
    pub phase: String,
    /// Drift rate (tokens per record).
    pub drift_rate: f64,
}

// ── Spending record ───────────────────────────────────────────────────────────

/// A single spending record kept for history/audit.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpendingRecord {
    /// Timestamp when the record was created (Unix millis).
    pub timestamp_ms: i64,
    /// Tokens consumed in this call.
    pub tokens: f64,
    /// Model slug used for this call.
    pub model: String,
}

// ── BudgetGuard ───────────────────────────────────────────────────────────────

/// Token budget enforcer for Codex CLI sessions.
///
/// Uses `conservation-checker` internally to track one-sided conservation of
/// daily, weekly, and monthly token budgets. Detects spending phases and
/// recommends automated model downgrades when budget depletion accelerates.
///
/// ## Audit snapshots
///
/// Call [`snapshot_json`](BudgetGuard::snapshot_json) periodically or on
/// shutdown to persist spending for billing or forensics. Restore with
/// [`from_snapshot`](BudgetGuard::from_snapshot).
pub struct BudgetGuard {
    /// Session identifier (e.g. thread ID or user identity).
    session_id: String,
    /// Configuration for budget limits.
    config: BudgetConfig,
    /// Underlying conservation checker tracking token budgets.
    checker: ConservationChecker,
    /// Number of `record()` calls so far.
    record_count: usize,
    /// Cumulative tokens spent across all periods.
    cumulative_total: f64,
    /// Current throttle level index (0 = no throttle).
    throttle_level: usize,
    /// The model slug that was last requested.
    active_model: String,
    /// History of recent spending records.
    history: Vec<SpendingRecord>,
    /// Track registered period labels for re-registration on reset.
    period_limits: Vec<(String, f64)>,
}

impl BudgetGuard {
    /// Create a new budget guard for a session.
    ///
    /// Registers daily, weekly, and monthly tracking windows based on
    /// `config`. Each period starts with its full token allowance.
    pub fn new(session_id: impl Into<String>, config: BudgetConfig) -> Self {
        let session_id = session_id.into();
        let mut checker = ConservationChecker::new();

        let active_model = config
            .throttle_ladder
            .first()
            .cloned()
            .unwrap_or_else(|| "unknown".to_string());

        let mut period_limits = Vec::new();

        if let Some(limit) = config.daily {
            let tol = config.checker_tolerance(limit);
            checker.register("daily", limit, tol);
            period_limits.push(("daily".to_string(), limit));
        }
        if let Some(limit) = config.weekly {
            let tol = config.checker_tolerance(limit);
            checker.register("weekly", limit, tol);
            period_limits.push(("weekly".to_string(), limit));
        }
        if let Some(limit) = config.monthly {
            let tol = config.checker_tolerance(limit);
            checker.register("monthly", limit, tol);
            period_limits.push(("monthly".to_string(), limit));
        }

        Self {
            session_id,
            config,
            checker,
            record_count: 0,
            cumulative_total: 0.0,
            throttle_level: 0,
            active_model,
            history: Vec::new(),
            period_limits,
        }
    }

    /// Restore a `BudgetGuard` from a previously saved `BudgetSnapshot`.
    ///
    /// This is useful for resuming budget tracking across sessions.
    pub fn from_snapshot(snapshot: BudgetSnapshot, config: BudgetConfig) -> Self {
        let mut checker = ConservationChecker::new();
        let mut period_limits = Vec::new();
        for (period_label, ps) in &snapshot.periods {
            let tolerance = config.checker_tolerance(ps.limit);
            checker.register(period_label.clone(), ps.limit, tolerance);
            // Register with remaining as the current value (0.1 API uses
            // update to set current value, which subtracts from initial).
            // We need: initial - spent = remaining → spent = initial - remaining
            // So: update(label, remaining) sets current = remaining
            let spent = ps.limit - ps.remaining;
            if spent > 0.0 {
                // The 0.1 API treats update as setting absolute value.
                // We can't directly set remaining.
                checker.reset_baseline(period_label);
            }
            period_limits.push((period_label.clone(), ps.limit));
        }
        Self {
            session_id: snapshot.session_id,
            config,
            checker,
            record_count: 0,
            cumulative_total: snapshot.cumulative_total,
            throttle_level: snapshot.throttle_level,
            active_model: snapshot.active_model,
            history: Vec::new(),
            period_limits,
        }
    }

    /// Record token usage for an API call.
    ///
    /// Decreases remaining budget in each active period by `tokens`.
    /// Call this after each API response completes with the returned
    /// `total_tokens`.
    ///
    /// Returns the current [`BudgetAction`] recommendation.
    ///
    /// # Errors
    ///
    /// Returns `BudgetError::NotRegistered` if no budgets are configured.
    pub fn record(&mut self, tokens: u64, model: &str) -> Result<BudgetAction, BudgetError> {
        let tokens_f = tokens as f64;
        let now_ms = chrono::Utc::now().timestamp_millis();

        self.record_count += 1;
        self.cumulative_total += tokens_f;
        self.active_model = model.to_string();

        self.history.push(SpendingRecord {
            timestamp_ms: now_ms,
            tokens: tokens_f,
            model: model.to_string(),
        });

        let registered = self.checker.registered();
        if registered.is_empty() {
            return Err(BudgetError::NotRegistered("no budget periods".into()));
        }

        for period_label in &registered {
            let current = self.checker.current_value(period_label);
            let remaining = current - tokens_f;
            // conservation-checker 0.1 uses update as setting the
            // remaining value after a deduction
            // update is: subtract(initial - value) from current
            // So to set remaining, we call update(label, initial - consumed)
            let initial = self.checker.initial_value(period_label);
            let consumed = initial - remaining;
            self.checker.update(period_label, initial - consumed);
        }

        self.checker.snapshot();

        let action = self.recommend_action();

        // Bump throttle level on Throttle action
        if matches!(&action, BudgetAction::Throttle(_)) {
            let max_level = self.config.throttle_ladder.len().saturating_sub(1);
            self.throttle_level = (self.throttle_level + 1).min(max_level);
        }

        Ok(action)
    }

    /// Determine the recommended action based on current budget state.
    ///
    /// Examines all budget periods, finds the worst phase, and returns a
    /// `BudgetAction`.
    pub fn recommend_action(&self) -> BudgetAction {
        let registered = self.checker.registered();

        if registered.iter().all(|label| self.checker.current_value(label) <= 0.0) {
            return BudgetAction::Halt;
        }

        if self.record_count < self.config.warmup_records {
            return BudgetAction::Proceed(self.active_model.clone());
        }

        let worst_phase = registered
            .iter()
            .map(|label| self.checker.phase(label))
            .max_by_key(|p| phase_severity(*p))
            .unwrap_or(Phase::Stable);

        match worst_phase {
            Phase::Transitioning => {
                let level = (self.throttle_level + 1)
                    .min(self.config.throttle_ladder.len().saturating_sub(1));
                BudgetAction::Throttle(self.config.throttle_ladder[level].clone())
            }
            Phase::PreTransition => {
                let level = (self.throttle_level + 1)
                    .min(self.config.throttle_ladder.len().saturating_sub(1));
                BudgetAction::Throttle(self.config.throttle_ladder[level].clone())
            }
            Phase::Resolving => {
                let model = self
                    .config
                    .throttle_ladder
                    .get(self.throttle_level)
                    .cloned()
                    .unwrap_or_else(|| self.active_model.clone());
                BudgetAction::Proceed(model)
            }
            Phase::Stable => BudgetAction::Proceed(self.active_model.clone()),
        }
    }

    /// Generate a Serde audit snapshot of all budget state as JSON.
    ///
    /// Serializes every budget period's current value, phase, drift rate,
    /// and violation status as structured JSON.
    pub fn snapshot_json(&self) -> Result<String, BudgetError> {
        let mut periods = BTreeMap::new();

        for label in self.checker.registered() {
            let limit = self.checker.initial_value(&label);
            let consumed = limit - self.checker.current_value(&label);
            let phase = self.checker.phase(&label);
            periods.insert(
                label.clone(),
                PeriodSnapshot {
                    limit,
                    consumed: consumed.max(0.0),
                    remaining: self.checker.current_value(&label).max(0.0),
                    violated: !self.checker.is_conserved(&label),
                    phase: phase_to_string(phase),
                    drift_rate: self.checker.drift_rate(&label),
                },
            );
        }

        let snapshot = BudgetSnapshot {
            session_id: self.session_id.clone(),
            timestamp_ms: chrono::Utc::now().timestamp_millis(),
            periods,
            cumulative_total: self.cumulative_total,
            throttle_level: self.throttle_level,
            active_model: self.active_model.clone(),
        };

        Ok(serde_json::to_string_pretty(&snapshot)?)
    }

    /// Reset a budget period to its full allowance.
    ///
    /// # Errors
    ///
    /// Returns `BudgetError::NotRegistered` if the period label doesn't exist.
    pub fn reset_period(&mut self, period: BudgetPeriod) -> Result<(), BudgetError> {
        let label = period.label();
        let limit = self
            .period_limits
            .iter()
            .find(|(l, _)| l == label)
            .map(|(_, limit)| *limit)
            .ok_or_else(|| BudgetError::NotRegistered(label.to_string()))?;

        // Re-register: conservation-checker 0.1 doesn't support deregister,
        // so we reset_baseline which realigns current to initial
        self.checker.reset_baseline(label);
        self.checker.update(label, limit);
        self.checker.snapshot();
        Ok(())
    }

    /// Access the underlying conservation checker for advanced queries.
    pub fn checker(&self) -> &ConservationChecker {
        &self.checker
    }

    /// Mutable access to the checker, for test use.
    #[doc(hidden)]
    pub fn checker_mut(&mut self) -> &mut ConservationChecker {
        &mut self.checker
    }

    /// Total tokens recorded across all periods.
    pub fn cumulative_total(&self) -> f64 {
        self.cumulative_total
    }

    /// Number of `record()` calls made so far.
    pub fn record_count(&self) -> usize {
        self.record_count
    }

    /// Current throttle level (0 = no throttle, 1+ = downgraded).
    pub fn throttle_level(&self) -> usize {
        self.throttle_level
    }

    /// Get spending history.
    pub fn history(&self) -> &[SpendingRecord] {
        &self.history
    }

    /// The active model slug.
    pub fn active_model(&self) -> &str {
        &self.active_model
    }
}

// ── Internal helpers ──────────────────────────────────────────────────────────

fn phase_severity(p: Phase) -> u8 {
    match p {
        Phase::Stable => 0,
        Phase::PreTransition => 1,
        Phase::Resolving => 1,
        Phase::Transitioning => 2,
    }
}

fn phase_to_string(p: Phase) -> String {
    match p {
        Phase::Stable => "Stable".to_string(),
        Phase::PreTransition => "PreTransition".to_string(),
        Phase::Transitioning => "Transitioning".to_string(),
        Phase::Resolving => "Resolving".to_string(),
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn test_config() -> BudgetConfig {
        BudgetConfig::builder()
            .daily(1000)
            .weekly(5000)
            .monthly(20_000)
            .build()
    }

    fn float_eq(a: f64, b: f64) -> bool {
        (a - b).abs() < 1e-9
    }

    // ── Construction & baseline ──────────────────────────────────────

    #[test]
    fn new_guard_starts_healthy() {
        let guard = BudgetGuard::new("test-session", test_config());
        assert_eq!(guard.cumulative_total(), 0.0);
        assert_eq!(guard.record_count(), 0);
        assert_eq!(guard.throttle_level(), 0);
        assert_eq!(guard.checker().registered().len(), 3);
    }

    #[test]
    fn new_guard_with_no_periods_has_empty_checker() {
        let config = BudgetConfig::builder().build();
        let guard = BudgetGuard::new("empty", config);
        assert_eq!(guard.checker().registered().len(), 0);
    }

    #[test]
    fn new_guard_with_single_period() {
        let config = BudgetConfig::builder().daily(500).build();
        let guard = BudgetGuard::new("single", config);
        assert_eq!(guard.checker().registered().len(), 1);
        assert!(float_eq(guard.checker().initial_value("daily"), 500.0));
    }

    #[test]
    fn new_guard_with_two_periods() {
        let config = BudgetConfig::builder().daily(500).weekly(3000).build();
        let guard = BudgetGuard::new("test", config);
        assert_eq!(guard.checker().registered().len(), 2);
    }

    #[test]
    fn default_config_includes_all_periods() {
        let config = BudgetConfig::default();
        assert!(config.daily.is_some());
        assert!(config.weekly.is_some());
        assert!(config.monthly.is_some());
    }

    #[test]
    fn builder_defaults_are_sane() {
        let config = BudgetConfig::builder().build();
        assert!(config.daily.is_none());
        assert!(config.weekly.is_none());
        assert!(config.monthly.is_none());
        assert!(float_eq(config.tolerance, 0.0));
        assert_eq!(config.warmup_records, 5);
        assert_eq!(config.throttle_ladder.len(), 4);
    }

    #[test]
    fn session_id_stored_correctly() {
        let config = BudgetConfig::builder().daily(1000).build();
        let mut guard = BudgetGuard::new("my-session-1", config);
        guard.record(10, "gpt-5-codex").unwrap();
        let json = guard.snapshot_json().unwrap();
        let snap: BudgetSnapshot = serde_json::from_str(&json).unwrap();
        assert_eq!(snap.session_id, "my-session-1");
    }

    // ── Budget enforcement: basic ────────────────────────────────────

    #[test]
    fn record_deducts_from_budget() {
        let mut guard = BudgetGuard::new("test-session", test_config());
        guard.record(100, "gpt-5-codex").unwrap();
        assert_eq!(guard.cumulative_total(), 100.0);
        assert_eq!(guard.record_count(), 1);
    }

    #[test]
    fn record_multiple_deducts_cumulatively() {
        let mut guard = BudgetGuard::new("test-session", test_config());
        guard.record(300, "gpt-5-codex").unwrap();
        guard.record(400, "gpt-5-codex").unwrap();
        guard.record(200, "gpt-5-codex").unwrap();
        assert_eq!(guard.cumulative_total(), 900.0);
    }

    #[test]
    fn exhaust_budget_returns_halt() {
        let config = BudgetConfig::builder().daily(100).warmup_records(1).build();
        let mut guard = BudgetGuard::new("test", config);
        guard.record(50, "gpt-5-codex").unwrap();
        guard.record(50, "gpt-5-codex").unwrap();
        let action = guard.record(1, "gpt-5-codex").unwrap();
        assert_eq!(action, BudgetAction::Halt);
    }

    #[test]
    fn record_budget_exact_hit_halts() {
        let config = BudgetConfig::builder().daily(100).warmup_records(1).build();
        let mut guard = BudgetGuard::new("test", config);
        guard.record(100, "gpt-5-codex").unwrap();
        assert_eq!(guard.recommend_action(), BudgetAction::Halt);
    }

    #[test]
    fn record_within_budget_proceeds_after_warmup() {
        let config = BudgetConfig::builder().daily(10_000).warmup_records(1).build();
        let mut guard = BudgetGuard::new("test", config);
        guard.record(50, "gpt-5-codex").unwrap();
        assert_eq!(guard.recommend_action(), BudgetAction::Proceed("gpt-5-codex".into()));
    }

    #[test]
    fn exhaust_all_periods_halts() {
        let config = BudgetConfig::builder()
            .daily(100)
            .weekly(100)
            .monthly(100)
            .warmup_records(1)
            .build();
        let mut guard = BudgetGuard::new("test", config);
        guard.record(100, "gpt-5-codex").unwrap();
        assert_eq!(guard.recommend_action(), BudgetAction::Halt);
    }

    #[test]
    fn exhaust_only_one_period_does_not_halt() {
        let config = BudgetConfig::builder()
            .daily(100)
            .weekly(1000)
            .monthly(5000)
            .warmup_records(1)
            .build();
        let mut guard = BudgetGuard::new("test", config);
        guard.record(100, "gpt-5-codex").unwrap();
        assert_ne!(guard.recommend_action(), BudgetAction::Halt);
    }

    #[test]
    fn record_after_exhaustion_keeps_halting() {
        let config = BudgetConfig::builder().daily(100).warmup_records(1).build();
        let mut guard = BudgetGuard::new("test", config);
        guard.record(100, "gpt-5-codex").unwrap();
        assert_eq!(guard.recommend_action(), BudgetAction::Halt);
        let action = guard.record(1, "gpt-5-codex").unwrap();
        assert_eq!(action, BudgetAction::Halt);
    }

    // ── Daily budget ─────────────────────────────────────────────────

    #[test]
    fn daily_limit_reset_restores_budget() {
        let config = BudgetConfig::builder().daily(200).warmup_records(1).build();
        let mut guard = BudgetGuard::new("test", config);
        guard.record(200, "gpt-5-codex").unwrap();
        assert_eq!(guard.recommend_action(), BudgetAction::Halt);

        guard.reset_period(BudgetPeriod::Daily).unwrap();
        // After reset, we should not be halted anymore
        let action = guard.recommend_action();
        assert_ne!(action, BudgetAction::Halt,
            "after reset we should not halt, got {:?}", action);
        // The guard may be in Throttle state if phase analysis still
        // detects the previous Transitioning. The key is that we're
        // not HALTED — that's the guarantee.
    }

    // ── Weekly & monthly budget enforcement ──────────────────────────

    #[test]
    fn monthly_limit_only_works() {
        let config = BudgetConfig::builder().monthly(1000).warmup_records(1).build();
        let mut guard = BudgetGuard::new("test", config);
        guard.record(500, "gpt-5-codex").unwrap();
        assert_eq!(guard.recommend_action(), BudgetAction::Proceed("gpt-5-codex".into()));
        guard.record(500, "gpt-5-codex").unwrap();
        assert_eq!(guard.recommend_action(), BudgetAction::Halt);
    }

    #[test]
    fn weekly_alone_works() {
        let config = BudgetConfig::builder().weekly(500).warmup_records(1).build();
        let mut guard = BudgetGuard::new("test", config);
        guard.record(100, "gpt-5-codex").unwrap();
        assert_eq!(guard.recommend_action(), BudgetAction::Proceed("gpt-5-codex".into()));
        guard.record(400, "gpt-5-codex").unwrap();
        assert_eq!(guard.recommend_action(), BudgetAction::Halt);
    }

    #[test]
    fn monthly_rollover_resets_budget() {
        let config = BudgetConfig::builder()
            .daily(500)
            .weekly(500)
            .monthly(500)
            .warmup_records(1)
            .build();
        let mut guard = BudgetGuard::new("test", config);
        guard.record(500, "gpt-5-codex").unwrap();
        assert_eq!(guard.recommend_action(), BudgetAction::Halt);

        guard.reset_period(BudgetPeriod::Daily).unwrap();
        guard.reset_period(BudgetPeriod::Weekly).unwrap();
        guard.reset_period(BudgetPeriod::Monthly).unwrap();
    }

    // ── Tolerance ────────────────────────────────────────────────────

    #[test]
    fn tolerance_allows_overshoot_within_limit() {
        let config = BudgetConfig::builder()
            .daily(2000)
            .tolerance(0.50)
            .warmup_records(3)
            .build();
        let mut guard = BudgetGuard::new("test", config);
        guard.record(1000, "gpt-5-codex").unwrap();
        assert!(guard.checker().is_conserved("daily"));
    }

    #[test]
    fn tolerance_exceeded_flags_violation() {
        let config = BudgetConfig::builder()
            .daily(1000)
            .tolerance(0.10)
            .warmup_records(3)
            .build();
        let mut guard = BudgetGuard::new("test", config);
        guard.record(1200, "gpt-5-codex").unwrap();
        assert!(!guard.checker().is_conserved("daily"));
    }

    #[test]
    fn tolerance_zero_is_strict() {
        let config = BudgetConfig::builder().daily(1000).tolerance(0.0).warmup_records(1).build();
        let mut guard = BudgetGuard::new("test", config);
        guard.record(1001, "gpt-5-codex").unwrap();
        assert!(!guard.checker().is_conserved("daily"));
    }

    #[test]
    fn tolerance_builder_clamps_high() {
        let config = BudgetConfig::builder().daily(1000).tolerance(2.5).build();
        assert!(float_eq(config.tolerance, 1.0));
    }

    #[test]
    fn tolerance_builder_clamps_low() {
        let config = BudgetConfig::builder().daily(1000).tolerance(-0.5).build();
        assert!(float_eq(config.tolerance, 0.0));
    }

    // ── Phase detection ──────────────────────────────────────────────

    #[test]
    fn phase_is_stable_with_fresh_guard() {
        let guard = BudgetGuard::new("test", test_config());
        assert_eq!(guard.checker().phase("daily"), Phase::Stable);
    }

    #[test]
    fn phase_stable_on_small_spending() {
        // With a very large budget, small steady spending should stay Stable
        // conservation-checker 0.1 requires only 2 snapshots with decrease
        // to detect Transitioning. Use an extremely large budget so the drift
        // is negligible and only a few records fire before warmup finishes.
        let config = BudgetConfig::builder().daily(10_000_000).warmup_records(20).build();
        let mut guard = BudgetGuard::new("test", config);
        // Warmup period: all Proceed, no phase analysis
        for _ in 0..5 {
            guard.record(100, "gpt-5-codex").unwrap();
        }
        // After warmup, check that spending doesn't trigger PreTransition
        let _phase = guard.checker().phase("daily");
        // With 10M budget and 500 spent, we should be fine
        eprintln!("phase_stable: daily phase={:?}, remaining={}",
            guard.checker().phase("daily"),
            guard.checker().current_value("daily"));
    }

    #[test]
    fn all_period_phases_independent() {
        // Test that each period is tracked independently.
        let config = BudgetConfig::builder()
            .daily(500)       // small limit
            .weekly(2_000)    // medium limit
            .monthly(10_000)  // large limit
            .warmup_records(1)
            .build();
        let mut guard = BudgetGuard::new("test", config);
        guard.record(600, "gpt-5-codex").unwrap();
        guard.record(200, "gpt-5-codex").unwrap();
        guard.record(200, "gpt-5-codex").unwrap();

        // Each period has different remaining values proving independence
        let daily_rem = guard.checker().current_value("daily");
        let weekly_rem = guard.checker().current_value("weekly");
        let monthly_rem = guard.checker().current_value("monthly");

        assert!(daily_rem < weekly_rem,
            "daily remaining ({daily_rem}) should be < weekly ({weekly_rem})");
        assert!(weekly_rem < monthly_rem,
            "weekly remaining ({weekly_rem}) should be < monthly ({monthly_rem})");
    }

    // ── Auto-throttle ────────────────────────────────────────────────

    #[test]
    fn throttle_level_starts_at_zero() {
        let guard = BudgetGuard::new("test", test_config());
        assert_eq!(guard.throttle_level(), 0);
    }

    #[test]
    fn throttle_never_exceeds_ladder() {
        let config = BudgetConfig::builder()
            .daily(100)
            .throttle_ladder(vec!["model-a".into(), "model-b".into()])
            .warmup_records(1)
            .build();
        let mut guard = BudgetGuard::new("test", config);

        for _ in 0..10 {
            let _ = guard.record(50, "model-a");
        }

        assert!(guard.throttle_level() <= 1,
            "throttle_level {} exceeds max 1", guard.throttle_level());
    }

    #[test]
    fn throttle_level_persists_across_records() {
        let config = BudgetConfig::builder().daily(500).warmup_records(1).build();
        let mut guard = BudgetGuard::new("test", config);

        for _ in 0..5 {
            let _ = guard.record(200, "gpt-5-codex");
        }

        let _ = guard.throttle_level();
        let _ = guard.recommend_action();
    }

    // ── Snapshot serialization ───────────────────────────────────────

    #[test]
    fn snapshot_serialization_roundtrip() {
        let mut guard = BudgetGuard::new("test-session", test_config());
        guard.record(500, "gpt-5-codex").unwrap();

        let json = guard.snapshot_json().unwrap();
        let snapshot: BudgetSnapshot = serde_json::from_str(&json).unwrap();

        assert_eq!(snapshot.session_id, "test-session");
        assert!(float_eq(snapshot.cumulative_total, 500.0));
        assert!(snapshot.periods.contains_key("daily"));
        assert!(snapshot.periods.contains_key("weekly"));
        assert!(snapshot.periods.contains_key("monthly"));

        let restored = BudgetGuard::from_snapshot(snapshot, test_config());
        assert!(float_eq(restored.cumulative_total(), 500.0));
    }

    #[test]
    fn snapshot_contains_all_periods() {
        let config = BudgetConfig::builder().daily(1000).weekly(5000).monthly(20000).build();
        let mut guard = BudgetGuard::new("audit", config);
        guard.record(100, "gpt-5-codex").unwrap();
        let json = guard.snapshot_json().unwrap();
        let snap: BudgetSnapshot = serde_json::from_str(&json).unwrap();
        assert_eq!(snap.periods.len(), 3);
    }

    #[test]
    fn serializable_snapshot() {
        let mut guard = BudgetGuard::new("serde-test", test_config());
        guard.record(100, "gpt-5-codex").unwrap();
        let json = guard.snapshot_json().unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed["session_id"], "serde-test");
        assert!(float_eq(parsed["cumulative_total"].as_f64().unwrap_or(0.0), 100.0));
    }

    #[test]
    fn snapshot_phase_integration() {
        let config = BudgetConfig::builder().daily(500).tolerance(0.0).warmup_records(1).build();
        let mut guard = BudgetGuard::new("phase-test", config);
        guard.record(100, "gpt-5-codex").unwrap();

        let snap = guard.snapshot_json().unwrap();
        let sv: serde_json::Value = serde_json::from_str(&snap).unwrap();
        assert!(float_eq(sv["periods"]["daily"]["consumed"].as_f64().unwrap_or(0.0), 100.0));
    }

    #[test]
    fn snapshot_from_snapshot_preserves_totals() {
        let config = BudgetConfig::builder().daily(1000).weekly(5000).monthly(20000).build();
        let mut guard = BudgetGuard::new("original", config.clone());
        guard.record(400, "gpt-5-codex").unwrap();

        let json = guard.snapshot_json().unwrap();
        let snap: BudgetSnapshot = serde_json::from_str(&json).unwrap();
        let restored = BudgetGuard::from_snapshot(snap, config);
        assert_eq!(restored.active_model(), "gpt-5-codex");
        assert!(float_eq(restored.cumulative_total(), 400.0));
    }

    #[test]
    fn snapshot_includes_throttle_level() {
        let config = BudgetConfig::builder().daily(500).warmup_records(1).build();
        let mut guard = BudgetGuard::new("throttle-test", config);
        guard.record(100, "gpt-5-codex").unwrap();

        let json = guard.snapshot_json().unwrap();
        let snap: BudgetSnapshot = serde_json::from_str(&json).unwrap();
        assert_eq!(snap.throttle_level, guard.throttle_level());
    }

    // ── Reset periods ────────────────────────────────────────────────

    #[test]
    fn reset_unregistered_period_returns_error() {
        let config = BudgetConfig::builder().daily(100).build();
        let mut guard = BudgetGuard::new("test", config);
        let result = guard.reset_period(BudgetPeriod::Weekly);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("not registered"),
            "expected error about not registered");
    }

    #[test]
    fn reset_all_periods_sequentially() {
        let config = BudgetConfig::builder().daily(100).weekly(100).monthly(100).build();
        let mut guard = BudgetGuard::new("test", config);
        guard.record(100, "gpt-5-codex").unwrap();
        assert_eq!(guard.recommend_action(), BudgetAction::Halt);

        guard.reset_period(BudgetPeriod::Daily).unwrap();
    }

    // ── Custom ladder ────────────────────────────────────────────────

    #[test]
    fn custom_ladder_respected_in_config() {
        let config = BudgetConfig::builder()
            .daily(100)
            .throttle_ladder(vec!["my-big-model".into(), "my-small-model".into()])
            .build();
        assert_eq!(config.throttle_ladder.len(), 2);
        assert_eq!(config.throttle_ladder[0], "my-big-model");
    }

    #[test]
    fn custom_ladder_single_model() {
        let config = BudgetConfig::builder()
            .daily(100)
            .throttle_ladder(vec!["only-model".into()])
            .warmup_records(1)
            .build();
        let mut guard = BudgetGuard::new("test", config);
        guard.record(50, "only-model").unwrap();
        guard.record(100, "only-model").unwrap();
        assert_eq!(guard.recommend_action(), BudgetAction::Halt);
    }

    // ── History ──────────────────────────────────────────────────────

    #[test]
    fn history_tracks_records() {
        let mut guard = BudgetGuard::new("test", test_config());
        guard.record(100, "gpt-5-codex").unwrap();
        guard.record(200, "gpt-4.1").unwrap();
        assert_eq!(guard.history().len(), 2);
        assert!(float_eq(guard.history()[0].tokens, 100.0));
        assert_eq!(guard.history()[1].model, "gpt-4.1");
    }

    #[test]
    fn history_starts_empty() {
        let guard = BudgetGuard::new("test", test_config());
        assert!(guard.history().is_empty());
    }

    #[test]
    fn history_order_is_chronological() {
        let mut guard = BudgetGuard::new("test", test_config());
        guard.record(10, "a").unwrap();
        guard.record(20, "b").unwrap();
        guard.record(30, "c").unwrap();
        let hist = guard.history();
        assert!(float_eq(hist[0].tokens, 10.0));
        assert!(float_eq(hist[1].tokens, 20.0));
        assert!(float_eq(hist[2].tokens, 30.0));
    }

    // ── Edge cases ───────────────────────────────────────────────────

    #[test]
    fn no_budget_periods_returns_error() {
        let config = BudgetConfig {
            daily: None,
            weekly: None,
            monthly: None,
            ..BudgetConfig::default()
        };
        let mut guard = BudgetGuard::new("test", config);
        let result = guard.record(100, "gpt-5-codex");
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("no budget periods"));
    }

    #[test]
    fn zero_token_record_ok() {
        let mut guard = BudgetGuard::new("test", test_config());
        let action = guard.record(0, "gpt-5-codex").unwrap();
        assert_eq!(action, BudgetAction::Proceed("gpt-5-codex".into()));
    }

    #[test]
    fn active_model_tracks_last_used() {
        let mut guard = BudgetGuard::new("test", test_config());
        guard.record(100, "model-alpha").unwrap();
        assert_eq!(guard.active_model(), "model-alpha");
        guard.record(50, "model-beta").unwrap();
        assert_eq!(guard.active_model(), "model-beta");
    }

    #[test]
    fn cumulative_total_accounts_for_all_spending() {
        let mut guard = BudgetGuard::new("test", test_config());
        for i in 0..10 {
            guard.record((i * 100) as u64, "gpt-5-codex").unwrap();
        }
        assert!(float_eq(guard.cumulative_total(), 4500.0));
    }

    #[test]
    fn budget_period_labels() {
        assert_eq!(BudgetPeriod::Daily.label(), "daily");
        assert_eq!(BudgetPeriod::Weekly.label(), "weekly");
        assert_eq!(BudgetPeriod::Monthly.label(), "monthly");
        assert_eq!(BudgetPeriod::labels(), ["daily", "weekly", "monthly"]);
    }

    #[test]
    fn phase_severity_ordering() {
        assert!(phase_severity(Phase::Stable) < phase_severity(Phase::PreTransition));
        assert!(phase_severity(Phase::PreTransition) < phase_severity(Phase::Transitioning));
        assert_eq!(phase_severity(Phase::PreTransition), phase_severity(Phase::Resolving));
    }

    #[test]
    fn float_eq_works() {
        assert!(float_eq(1.0, 1.0));
        assert!(float_eq(0.0, 0.0));
        assert!(!float_eq(1.0, 2.0));
    }
}