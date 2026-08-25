//! Runtime policy and execution budgets.

/// Runtime execution budget used by the top-level facade.
#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub struct RuntimeBudget {
    pub tick_limit: usize,
    pub async_poll_limit: usize,
}

impl RuntimeBudget {
    /// Create a conservative default budget for early boot.
    pub const fn early_boot() -> Self {
        Self {
            tick_limit: 1_000,
            async_poll_limit: 64,
        }
    }
}

/// Top-level runtime policy.
#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub struct RuntimePolicy {
    pub budget: RuntimeBudget,
    pub cooperative_only: bool,
}

impl RuntimePolicy {
    /// Build the default early-boot policy.
    pub const fn early_boot() -> Self {
        Self {
            budget: RuntimeBudget::early_boot(),
            cooperative_only: true,
        }
    }
}
