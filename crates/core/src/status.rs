//! Where a token entry stands, how it arrived, and how its provider charges.

use serde::{Deserialize, Serialize};

/// Where a token entry stands.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    /// Still on the machine, waiting for its token sheet.
    Draft,
    /// Counts in reports. Automatic token entries start here.
    Submitted,
    /// A people manager accepted it. Only used when approval is on.
    Approved,
    /// Sent back to its agent manager with a reason. Does not count until
    /// fixed.
    Rejected,
    /// Its period is closed.
    Locked,
}

impl Status {
    /// Whether a token entry in this status counts on the cost sheet. With
    /// approval on, only approved cost counts; a locked entry was counted
    /// when its period closed, so it always counts.
    #[must_use]
    pub const fn counts(self, approval_on: bool) -> bool {
        match self {
            Self::Draft | Self::Rejected => false,
            Self::Submitted => !approval_on,
            Self::Approved | Self::Locked => true,
        }
    }

    /// Whether a token entry in this status counts in reports. Submitted
    /// cost counts at once, with approval on too, where reports mark it as
    /// not yet approved.
    #[must_use]
    pub const fn counts_in_reports(self) -> bool {
        !matches!(self, Self::Draft | Self::Rejected)
    }
}

/// How a token entry reached Token Controller.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Arrival {
    /// With no person involved: cloud, CI and support agents, and token
    /// entries built from telemetry. Never on a token sheet.
    Automatic,
    /// Reviewed and submitted by its agent manager on a token sheet.
    TokenSheet,
}

/// One way a provider charges an organization.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CostSource {
    Subscription,
    Seat,
    UsageAtApiRates,
    ApiKey,
    CloudMarketplace,
    Reseller,
}

impl CostSource {
    /// Whether the provider charges per use, so a bill exists to reconcile
    /// against. Seats and subscriptions are flat fees: usage inside them is
    /// shown at API prices as an estimate, and reports can switch to the
    /// share view.
    #[must_use]
    pub const fn is_metered(self) -> bool {
        !matches!(self, Self::Subscription | Self::Seat)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn approval_decides_whether_submitted_cost_counts() {
        assert!(Status::Submitted.counts(false));
        assert!(!Status::Submitted.counts(true));
        assert!(Status::Submitted.counts_in_reports());
        assert!(Status::Approved.counts_in_reports());
        assert!(Status::Locked.counts_in_reports());
        assert!(!Status::Draft.counts_in_reports());
        assert!(!Status::Rejected.counts_in_reports());
        for approval_on in [false, true] {
            assert!(!Status::Draft.counts(approval_on));
            assert!(!Status::Rejected.counts(approval_on));
            assert!(Status::Approved.counts(approval_on));
            assert!(Status::Locked.counts(approval_on));
        }
    }

    #[test]
    fn only_seats_and_subscriptions_have_no_bill() {
        assert!(!CostSource::Seat.is_metered());
        assert!(!CostSource::Subscription.is_metered());
        for metered in [
            CostSource::UsageAtApiRates,
            CostSource::ApiKey,
            CostSource::CloudMarketplace,
            CostSource::Reseller,
        ] {
            assert!(metered.is_metered(), "{metered:?}");
        }
        assert_eq!(
            serde_json::to_string(&CostSource::UsageAtApiRates).unwrap(),
            "\"usage_at_api_rates\""
        );
    }
}
