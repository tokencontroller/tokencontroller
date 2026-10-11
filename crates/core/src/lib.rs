//! The accounting core shared by the service and the `tc` command (ADR 0010).
//!
//! A pure library: no database, no network, no clock. Everything that has to
//! add up (money, splits, reconciliation, rules) lives here so the service and
//! the command line compute the same answer.

pub mod extension;
pub mod money;

pub use extension::{Extension, NoExtension};
pub use money::{Currency, Money, MoneyError};
