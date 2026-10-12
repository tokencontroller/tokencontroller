//! A bill as a pull or an upload reads it, before anything is stored.

use std::fmt;

use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use time::Date;
use tokencontroller_core::{Currency, Money, MoneyError};

use super::period::{CostSource, Period};

/// The provider a bill comes from, spelled as bills are stored.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BillProvider {
    Anthropic,
    #[serde(rename = "openai")]
    OpenAi,
    Google,
    AwsBedrock,
    GcpVertex,
    AzureFoundry,
    Other,
}

impl BillProvider {
    /// The provider's name as stored, such as `aws_bedrock`.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Anthropic => "anthropic",
            Self::OpenAi => "openai",
            Self::Google => "google",
            Self::AwsBedrock => "aws_bedrock",
            Self::GcpVertex => "gcp_vertex",
            Self::AzureFoundry => "azure_foundry",
            Self::Other => "other",
        }
    }

    /// Reads a provider name as a person may type it: the stored names and
    /// a few common ones such as `Bedrock` or `Vertex AI`, ignoring case,
    /// spaces and hyphens. `None` for a name Token Controller does not know.
    #[must_use]
    pub fn from_name(name: &str) -> Option<Self> {
        let key: String = name
            .trim()
            .chars()
            .map(|c| match c {
                ' ' | '-' => '_',
                other => other.to_ascii_lowercase(),
            })
            .collect();
        match key.as_str() {
            "anthropic" | "anthropic_console" | "anthropic_enterprise" | "claude" => {
                Some(Self::Anthropic)
            }
            "openai" | "open_ai" => Some(Self::OpenAi),
            "google" | "gemini" => Some(Self::Google),
            "aws_bedrock" | "bedrock" | "amazon_bedrock" => Some(Self::AwsBedrock),
            "gcp_vertex" | "vertex" | "vertex_ai" | "google_vertex" => Some(Self::GcpVertex),
            "azure_foundry" | "foundry" | "microsoft_foundry" | "azure_openai" => {
                Some(Self::AzureFoundry)
            }
            "other" => Some(Self::Other),
            _ => None,
        }
    }
}

impl fmt::Display for BillProvider {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Where a bill comes from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BillOrigin {
    pub provider: BillProvider,
    pub cost_source: CostSource,
    /// The API or file format the bill was read from, such as
    /// `anthropic_console`, `openai_costs` or `csv`.
    pub source_api: &'static str,
    /// What the bill covers within that source: the provider's organization
    /// or billing account ID, or for a typed-in CSV bill its provider and
    /// cost source.
    pub scope: String,
}

/// One line of a bill: what one day cost for one workspace, key, model or
/// kind of usage, as the provider breaks it down. A bill's lines add up to
/// its gross.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BillLine {
    pub date: Date,
    /// The Anthropic workspace, OpenAI project or cloud account or project,
    /// if the provider names one. `None` for Anthropic's default workspace.
    pub workspace: Option<String>,
    pub api_key_id: Option<String>,
    pub model: Option<String>,
    /// The provider's own words for the line, such as
    /// `Claude Sonnet 4.5 Usage - Input Tokens`.
    pub description: String,
    pub amount: Money,
}

/// The four amounts of a bill, before they are checked.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BillFigures {
    pub gross: Money,
    pub discount: Money,
    pub credit: Money,
    pub net: Money,
}

impl BillFigures {
    /// The figures of a bill whose net follows from the other three.
    ///
    /// # Errors
    ///
    /// [`BillError::Money`] if the amounts are in different currencies or
    /// the net does not fit.
    pub fn from_gross(gross: Money, discount: Money, credit: Money) -> Result<Self, BillError> {
        let net = gross.checked_sub(discount)?.checked_sub(credit)?;
        Ok(Self {
            gross,
            discount,
            credit,
            net,
        })
    }
}

/// Why a bill's figures do not hold together.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum BillError {
    #[error(
        "gross {gross} minus discount {discount} minus credit {credit} is {expected}, but net is {net}"
    )]
    DoesNotAddUp {
        gross: Money,
        discount: Money,
        credit: Money,
        net: Money,
        expected: Money,
    },
    #[error("the bill's lines add up to {lines}, but its gross is {gross}")]
    LinesDoNotAddUp { lines: Money, gross: Money },
    #[error(transparent)]
    Money(#[from] MoneyError),
}

/// A bill for one period from one provider and cost source, as a pull or an
/// upload read it. Its figures always hold together: gross − discount −
/// credit = net, all in one currency, and its lines add up to its gross.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PulledBill {
    origin: BillOrigin,
    period: Period,
    figures: BillFigures,
    lines: Vec<BillLine>,
    is_final: bool,
    exchange_rate: Option<Decimal>,
}

impl PulledBill {
    /// Checks a bill's figures and lines. A bill without lines, such as
    /// one typed in or uploaded, needs only its figures.
    ///
    /// # Errors
    ///
    /// [`BillError::DoesNotAddUp`] unless gross − discount − credit = net,
    /// [`BillError::LinesDoNotAddUp`] if there are lines and they do not
    /// add up to gross, and [`BillError::Money`] if any amount or line is in
    /// another currency than gross.
    pub fn new(
        origin: BillOrigin,
        period: Period,
        figures: BillFigures,
        lines: Vec<BillLine>,
        is_final: bool,
    ) -> Result<Self, BillError> {
        let BillFigures {
            gross,
            discount,
            credit,
            net,
        } = figures;
        let expected = gross.checked_sub(discount)?.checked_sub(credit)?;
        let net_in_currency = net.checked_add(Money::zero(gross.currency()))?;
        if expected != net_in_currency {
            return Err(BillError::DoesNotAddUp {
                gross,
                discount,
                credit,
                net,
                expected,
            });
        }
        if !lines.is_empty() {
            let total = Money::sum(lines.iter().map(|line| line.amount), gross.currency())?;
            if total != gross {
                return Err(BillError::LinesDoNotAddUp {
                    lines: total,
                    gross,
                });
            }
        }
        Ok(Self {
            origin,
            period,
            figures,
            lines,
            is_final,
            exchange_rate: None,
        })
    }

    /// A bill whose gross is the sum of its lines and whose net is gross
    /// less `discount` and `credit`, in the currency of `discount`.
    pub(super) fn from_lines(
        origin: BillOrigin,
        period: Period,
        lines: Vec<BillLine>,
        discount: Money,
        credit: Money,
        is_final: bool,
    ) -> Result<Self, BillError> {
        let gross = Money::sum(lines.iter().map(|line| line.amount), discount.currency())?;
        let figures = BillFigures::from_gross(gross, discount, credit)?;
        Self::new(origin, period, figures, lines, is_final)
    }

    /// The same bill with the exchange rate to the reporting currency that
    /// came with it.
    #[must_use]
    pub fn with_exchange_rate(mut self, rate: Decimal) -> Self {
        self.exchange_rate = Some(rate);
        self
    }

    #[must_use]
    pub fn provider(&self) -> BillProvider {
        self.origin.provider
    }

    #[must_use]
    pub fn cost_source(&self) -> CostSource {
        self.origin.cost_source
    }

    /// The API or file format the bill was read from, such as
    /// `anthropic_enterprise`, `anthropic_console`, `openai_costs` or `csv`.
    #[must_use]
    pub fn source_api(&self) -> &'static str {
        self.origin.source_api
    }

    /// A stable ID for this bill at its source: the source API, what it
    /// covers there and the period, such as
    /// `anthropic_console:12345678-1234-5678-1234-567812345678:2026-09`.
    /// Pulling or uploading the same bill again gives the same ID, so a
    /// second import is a revision, not a second bill.
    #[must_use]
    pub fn source_id(&self) -> String {
        format!(
            "{}:{}:{}",
            self.origin.source_api, self.origin.scope, self.period
        )
    }

    #[must_use]
    pub fn period(&self) -> Period {
        self.period
    }

    /// The currency the provider charges in.
    #[must_use]
    pub fn currency(&self) -> Currency {
        self.figures.gross.currency()
    }

    #[must_use]
    pub fn gross(&self) -> Money {
        self.figures.gross
    }

    #[must_use]
    pub fn discount(&self) -> Money {
        self.figures.discount
    }

    #[must_use]
    pub fn credit(&self) -> Money {
        self.figures.credit
    }

    #[must_use]
    pub fn net(&self) -> Money {
        self.figures.net
    }

    #[must_use]
    pub fn lines(&self) -> &[BillLine] {
        &self.lines
    }

    /// Whether the provider will no longer change these figures. Until then
    /// a later pull may restate the bill.
    #[must_use]
    pub fn is_final(&self) -> bool {
        self.is_final
    }

    /// The exchange rate to the reporting currency given with an uploaded
    /// bill, if any. Token Controller fills it in when it is left out.
    #[must_use]
    pub fn exchange_rate(&self) -> Option<Decimal> {
        self.exchange_rate
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use time::macros::date;

    fn usd(nanos: i64) -> Money {
        Money::from_nanos(nanos, Currency::USD)
    }

    fn line(amount: Money) -> BillLine {
        BillLine {
            date: date!(2026 - 09 - 01),
            workspace: None,
            api_key_id: None,
            model: None,
            description: "usage".to_owned(),
            amount,
        }
    }

    fn origin() -> BillOrigin {
        BillOrigin {
            provider: BillProvider::Anthropic,
            cost_source: CostSource::ApiKey,
            source_api: "anthropic_console",
            scope: "org_1".to_owned(),
        }
    }

    fn bill(figures: BillFigures, lines: Vec<BillLine>) -> Result<PulledBill, BillError> {
        PulledBill::new(
            origin(),
            Period::parse("2026-09").unwrap(),
            figures,
            lines,
            true,
        )
    }

    #[test]
    fn gross_less_discount_and_credit_is_net() {
        let figures = BillFigures {
            gross: usd(1000),
            discount: usd(100),
            credit: usd(50),
            net: usd(850),
        };
        let bill = bill(figures, vec![line(usd(600)), line(usd(400))]).unwrap();
        assert_eq!(bill.gross(), usd(1000));
        assert_eq!(bill.discount(), usd(100));
        assert_eq!(bill.credit(), usd(50));
        assert_eq!(bill.net(), usd(850));
        assert_eq!(bill.currency(), Currency::USD);
        assert_eq!(bill.lines().len(), 2);
        assert!(bill.is_final());
        assert_eq!(bill.exchange_rate(), None);
        assert_eq!(
            BillFigures::from_gross(usd(1000), usd(100), usd(50)).unwrap(),
            figures
        );
    }

    #[test]
    fn figures_that_do_not_add_up_are_refused() {
        let figures = BillFigures {
            gross: usd(1000),
            discount: usd(100),
            credit: usd(0),
            net: usd(950),
        };
        assert_eq!(
            bill(figures, Vec::new()),
            Err(BillError::DoesNotAddUp {
                gross: usd(1000),
                discount: usd(100),
                credit: usd(0),
                net: usd(950),
                expected: usd(900),
            })
        );
        let off_by_one = BillFigures {
            net: usd(901),
            ..figures
        };
        assert!(bill(off_by_one, Vec::new()).is_err());
        let exact = BillFigures {
            net: usd(900),
            ..figures
        };
        assert!(bill(exact, Vec::new()).is_ok());
        let with_credit = BillFigures {
            credit: usd(1),
            net: usd(899),
            ..figures
        };
        assert!(bill(with_credit, Vec::new()).is_ok());
    }

    #[test]
    fn lines_must_add_up_to_gross_in_its_currency() {
        let figures = BillFigures::from_gross(usd(1000), usd(0), usd(0)).unwrap();
        assert_eq!(
            bill(figures, vec![line(usd(999))]),
            Err(BillError::LinesDoNotAddUp {
                lines: usd(999),
                gross: usd(1000),
            })
        );
        let euro_line = line(Money::from_nanos(1000, Currency::EUR));
        assert!(matches!(
            bill(figures, vec![euro_line]),
            Err(BillError::Money(MoneyError::CurrencyMismatch { .. }))
        ));
        let euro_net = BillFigures {
            net: Money::from_nanos(1000, Currency::EUR),
            ..figures
        };
        assert!(matches!(
            bill(euro_net, Vec::new()),
            Err(BillError::Money(MoneyError::CurrencyMismatch { .. }))
        ));
    }

    #[test]
    fn a_bill_from_lines_adds_them_up() {
        let bill = PulledBill::from_lines(
            origin(),
            Period::parse("2026-09").unwrap(),
            vec![line(usd(700)), line(usd(300))],
            usd(100),
            usd(50),
            false,
        )
        .unwrap();
        assert_eq!(bill.gross(), usd(1000));
        assert_eq!(bill.net(), usd(850));
        assert!(!bill.is_final());
        let empty = PulledBill::from_lines(
            origin(),
            Period::parse("2026-09").unwrap(),
            Vec::new(),
            Money::zero(Currency::EUR),
            Money::zero(Currency::EUR),
            true,
        )
        .unwrap();
        assert_eq!(empty.currency(), Currency::EUR);
        assert!(empty.net().is_zero());
    }

    #[test]
    fn the_message_names_all_four_amounts() {
        let figures = BillFigures {
            gross: Money::from_minor_units(100_000, Currency::USD).unwrap(),
            discount: Money::from_minor_units(10_000, Currency::USD).unwrap(),
            credit: Money::zero(Currency::USD),
            net: Money::from_minor_units(95_000, Currency::USD).unwrap(),
        };
        let error = bill(figures, Vec::new()).unwrap_err();
        assert_eq!(
            error.to_string(),
            "gross 1000.00 USD minus discount 100.00 USD minus credit 0.00 USD is 900.00 USD, \
             but net is 950.00 USD"
        );
    }

    #[test]
    fn providers_read_and_write_as_their_stored_names() {
        for (provider, name) in [
            (BillProvider::Anthropic, "anthropic"),
            (BillProvider::OpenAi, "openai"),
            (BillProvider::Google, "google"),
            (BillProvider::AwsBedrock, "aws_bedrock"),
            (BillProvider::GcpVertex, "gcp_vertex"),
            (BillProvider::AzureFoundry, "azure_foundry"),
            (BillProvider::Other, "other"),
        ] {
            let json = serde_json::to_string(&provider).unwrap();
            assert_eq!(json, format!("\"{name}\""));
            assert_eq!(
                serde_json::from_str::<BillProvider>(&json).unwrap(),
                provider
            );
            assert_eq!(provider.to_string(), name);
            assert_eq!(BillProvider::from_name(name), Some(provider));
        }
        assert_eq!(
            BillProvider::from_name(" Anthropic-Console "),
            Some(BillProvider::Anthropic)
        );
        assert_eq!(
            BillProvider::from_name("Vertex AI"),
            Some(BillProvider::GcpVertex)
        );
        assert_eq!(
            BillProvider::from_name("Bedrock"),
            Some(BillProvider::AwsBedrock)
        );
        assert_eq!(BillProvider::from_name("Mistral"), None);
        assert_eq!(BillProvider::from_name(""), None);
    }

    #[test]
    fn the_source_id_names_api_scope_and_period() {
        let figures = BillFigures::from_gross(usd(5), usd(0), usd(0)).unwrap();
        let bill = bill(figures, Vec::new())
            .unwrap()
            .with_exchange_rate(Decimal::new(92, 2));
        assert_eq!(bill.source_id(), "anthropic_console:org_1:2026-09");
        assert_eq!(bill.source_api(), "anthropic_console");
        assert_eq!(bill.provider(), BillProvider::Anthropic);
        assert_eq!(bill.exchange_rate(), Some(Decimal::new(92, 2)));
        assert_eq!(bill.cost_source(), CostSource::ApiKey);
        assert_eq!(bill.period().to_string(), "2026-09");
    }
}
