//! The accounting core shared by the service and the `tc` command: the data
//! model of the picture "How costs connect" (docs: How it works, Reference >
//! Data model, ADR 0028) and nothing else.
//!
//! A pure library: no database, no network, no clock. The organization is
//! an append-only log of [`Event`]s. Nothing is edited or deleted; a
//! correction is a new event that supersedes or reverses an older one.
//! Every number is computed when you look, as a fold of the log up to a
//! moment ("as of"): [`State::fold`]. As of now gives the current figures;
//! as of the cutoff gives the frozen ones. Reports show their as-of moment.
//!
//! # Records
//!
//! Source records, added by anyone or any source, never changed: [`Message`],
//! [`Bill`], [`Estimate`], [`TagValueRecord`], [`Share`], [`PriceRow`],
//! [`AttributeValue`] (with [`PersonAttribute`] to put one on a person).
//! Assignments, each superseding an older one on the same target:
//! [`Split`], [`Accept`], [`Grant`]. Other events: the cutoff, [`Submit`]
//! and [`Approval`], [`Rule`], and a reversal of any event.
//!
//! # Formulas
//!
//! Tier 1, cost accounting:
//!
//! 1. `C(e) = Σ_kind tokens_kind(e) × p(provider, model, kind, as of t(e))`
//!    over the kinds that have a price row, at the contract's price if it
//!    has one, else ours: [`PriceTable::cost`], [`State::message_costs`].
//!    The provider is the contract's, else the one the sender reported; it
//!    is never guessed from the model. No counts is a count error, a
//!    message without a certain price is not priced yet; neither is ever
//!    zero.
//! 2. `T*(s)`: the tags of a split closed under defaults; a feature brings
//!    its project, a project its client, the person their attributes, a
//!    team its department: [`Values::close`], [`Line::tags`].
//! 3. `C(F, w) = Σ fraction(s) × C(e(s))` over splits with `F ⊆ T*(s)` and
//!    `t(e) ∈ w`: [`State::cost`]. The only formula reports use.
//! 4. The check of a bill of contract `c` over range `r`: metered,
//!    `Bill = C({contract: c}, r) + U`, `U ≥ 0`; flat, `Bill = fee + extra
//!    usage`. Beside it always: messages of the same provider with no
//!    contract, and those whose contract came from the fallback:
//!    [`State::reconcile`]. `U < 0` means a message is wrong:
//!    [`Reconciliation::a_message_is_wrong`].
//! 5. For one tag name the values partition the total: [`State::by_tag`].
//!    Labels direct, project overhead and organization overhead:
//!    [`State::by_label`].
//!
//! Tier 2, management control, reads tier 1 through `C(F, w)` only:
//!
//! 6. `Ĉ(w) = f(description, H)`: `f` is any estimator; `H` is
//!    [`State::history`].
//! 7. `P(F, w)`: the accepted estimate's amount, markup inside, spread over
//!    `w` by days of its range: [`State::plan`].
//! 8. `P(person, w) = Σ shares: fraction × P(project, w ∩ [from, to])`:
//!    [`State::person_plan`]; sums over 1 are shown: [`State::share_sums`].
//! 9. `Δ(F, w) = P(F, w) − C(F, w)`, positive is favourable:
//!    [`State::deviation`].
//! 10. A forecast over past windows, a report not a record:
//!     [`State::forecast`].
//!
//! Access: `can(person, capability, F)` from grants: [`State::can`], with
//! [`Role`] as the named bundles.
//!
//! Money is a whole number of nano-units (ADR 0004); fractions divide with
//! [`Money::allocate`] so parts always add up.

pub mod access;
pub mod activity;
pub mod bill;
pub mod contract;
pub mod estimate;
pub mod event;
pub mod extension;
pub mod message;
pub mod money;
pub mod pattern;
pub mod plan;
pub mod price;
pub mod rules;
pub mod split;
pub mod state;
pub mod submit;
pub mod tags;
pub mod tokens;
pub mod values;

pub use access::{Capability, Grant, Role, can};
pub use activity::Activity;
pub use bill::{Bill, Reconciliation};
pub use contract::{ContractAssignment, ContractBy, ContractKind, ContractTerms};
pub use estimate::{Accept, BASIS_POINTS, Band, Distribution, DistributionError, Estimate};
pub use event::{DateRange, Event, EventId, EventIdError, RangeError, Record};
pub use extension::{Extension, NoExtension};
pub use message::{
    BUILT_ID_PREFIX, IdError, Message, MessageId, ModelRef, SessionFacts, SessionId, Signals,
};
pub use money::{Currency, ExchangeRate, Money, MoneyError};
pub use pattern::{PatternError, TextPattern};
pub use plan::{Share, ShareSum};
pub use price::{CostLabel, MessageCost, PriceRow, PriceSource, PriceTable, date_of};
pub use rules::{Rule, Scope, Signal, Then};
pub use split::{AssignedBy, Label, Split, SplitError, SplitPart, SplitTarget};
pub use state::{
    Cost, Deviation, DeviationLabel, FoldError, Line, MessageCostLine, MessageKey, Recorded, State,
    TagValueHistory,
};
pub use submit::{Approval, Decision, Submission, SubmissionStatus, Submit};
pub use tags::{Filter, TagError, TagName, TagValue, Tags};
pub use tokens::{TokenCounts, TokenKind, TokenKindError};
pub use values::{AttributeValue, PersonAttribute, TagValueRecord, Values};
