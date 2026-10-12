//! Golden cases: one folder per docs scenario in `tests/golden`. Each case
//! describes an organization, its token entries and its bills as CSV, and
//! holds what the core must make of them: the splits, the cost sheet and,
//! where the scenario has them, reports, token budgets and the close check.
//! `tests/golden/README.md` describes the files.
//!
//! Every output this test computes is also written to
//! `target/tmp/golden/<case>/`, so a new case's expected files can be
//! checked by hand against the docs and then copied in.

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};

use serde::de::DeserializeOwned;
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;
use tokencontroller_core::{
    Adjustment, Bill, BlockRequest, CloseCheck, CostSheet, CostSheetRequest, Currency,
    ExchangeRate, Filter, HandShare, History, Message, MessageId, MessageSignals, ModelPrice,
    ModelRef, Money, Organization, Period, Person, PriceTable, Project, Rule, Segment, SessionId,
    Signals, Split, Status, TagMap, TagName, TagValue, Tags, Ticket, TicketKeyPattern, TokenBudget,
    TokenCounts, TokenEntry, Usage, Version, actual, group_by,
};

type Row = BTreeMap<String, String>;
type Outcome<T> = Result<T, String>;

#[test]
fn golden_cases_match_their_expected_files() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/golden");
    let mut cases: Vec<PathBuf> = fs::read_dir(&root)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| path.is_dir())
        .collect();
    cases.sort();
    assert!(!cases.is_empty(), "no golden cases in {}", root.display());
    let out = Path::new(env!("CARGO_TARGET_TMPDIR")).join("golden");
    let mut failures = Vec::new();
    for case in &cases {
        let name = case.file_name().unwrap().to_string_lossy().into_owned();
        match run(case, &out.join(&name)) {
            Ok(differences) => failures.extend(
                differences
                    .into_iter()
                    .map(|difference| format!("{name}: {difference}")),
            ),
            Err(error) => failures.push(format!("{name}: {error}")),
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n\n"));
}

/// Runs one case and returns how its outputs differ from what it expects.
fn run(case: &Path, out: &Path) -> Outcome<Vec<String>> {
    let settings: BTreeMap<String, String> = rows(case, "organization.csv")?
        .ok_or("organization.csv is missing")?
        .into_iter()
        .map(|row| Ok((cell(&row, "setting")?, cell(&row, "value")?)))
        .collect::<Outcome<_>>()?;
    let setting = |name: &str| settings.get(name).map_or("", String::as_str);
    let period = Period::parse(setting("period")).map_err(|error| error.to_string())?;
    let organization = organization(case, &settings)?;
    let prices = price_table(case, setting("price_version"))?;
    let entries = token_entries(case, prices.currency())?;

    let mut outputs: Vec<(&str, String)> = Vec::new();
    let mut splits: Vec<(String, Status, Period, Vec<Split>)> = Vec::new();
    let mut shown = String::from("entry,tags,amount,placed_by,notes\n");
    for (id, bill, entry) in &entries {
        let result = entry
            .split(&organization, &prices, None)
            .map_err(|error| format!("entry {id}: {error}"))?;
        for split in &result.splits {
            let mut notes: Vec<String> = Vec::new();
            if split.ticket_not_synced {
                notes.push("ticket not synced".to_owned());
            }
            notes.extend(
                split
                    .value
                    .unknown_models
                    .iter()
                    .map(|model| format!("unknown model {model}")),
            );
            notes.extend(result.dropped.iter().map(|name| format!("dropped {name}")));
            let _ignored = writeln!(
                shown,
                "{},{},{},{},{}",
                id,
                tag_list(&split.tags),
                exact(split.value.list),
                split.placed_by,
                notes.join("; ")
            );
        }
        splits.push((bill.clone(), entry.status, entry.period(), result.splits));
    }
    outputs.push(("splits.csv", shown));

    let sheet = cost_sheet(case, &organization, period, &settings, &splits)?;
    outputs.push(("cost_sheet.csv", sheet));

    let in_reports: Vec<(&Tags, Money)> = splits
        .iter()
        .filter(|(_, status, entry_period, _)| {
            status.counts_in_reports() && *entry_period == period
        })
        .flat_map(|(_, _, _, splits)| splits)
        .map(|split| (&split.tags, split.value.list))
        .collect();
    if let Some(reports) = reports(case, &in_reports, prices.currency())? {
        outputs.push(("reports.csv", reports));
    }
    if let Some(budgets) = budgets(case, &in_reports, period, prices.currency())? {
        outputs.push(("budgets.csv", budgets));
    }
    if case.join("close.csv").exists() {
        let check = CloseCheck::of(
            period,
            organization.approval,
            entries.iter().map(|(_, _, entry)| entry),
        );
        outputs.push((
            "close.csv",
            format!(
                "unapproved,without_agent_manager,rejected,can_close\n{},{},{},{}\n",
                check.unapproved,
                check.without_agent_manager,
                check.rejected,
                check.can_close()
            ),
        ));
    }

    fs::create_dir_all(out).map_err(|error| error.to_string())?;
    let mut differences = Vec::new();
    for (file, actual) in outputs {
        fs::write(out.join(file), &actual).map_err(|error| error.to_string())?;
        let expected = fs::read_to_string(case.join(file))
            .map_err(|error| format!("{file}: {error}"))?
            .replace("\r\n", "\n");
        if expected.trim_end() != actual.trim_end() {
            differences.push(format!(
                "{file} differs\n--- expected\n{expected}\n--- actual\n{actual}"
            ));
        }
    }
    Ok(differences)
}

/// The organization: settings, people, projects, tickets and rules.
fn organization(case: &Path, settings: &BTreeMap<String, String>) -> Outcome<Organization> {
    let setting = |name: &str| settings.get(name).map_or("", String::as_str);
    let currency =
        Currency::new(setting("reporting_currency")).map_err(|error| error.to_string())?;
    let mut organization = Organization::new(currency, word(setting("cost_source"))?);
    organization.approval = setting("approval") == "true";
    organization.tag_names = list(setting("tag_names"))
        .map(name)
        .collect::<Outcome<_>>()?;
    let cost_center = match setting("cost_center") {
        "" => TagName::person(),
        text => name(text)?,
    };
    let ladder = match setting("ladder") {
        "" => TagMap::default().ladder().to_vec(),
        text => list(text).map(name).collect::<Outcome<_>>()?,
    };
    organization.tag_map = TagMap::new(cost_center, ladder).map_err(|error| error.to_string())?;
    if !setting("several_keys").is_empty() {
        organization.several_keys = word(setting("several_keys"))?;
    }
    organization.missing_tickets = list(setting("missing_tickets"))
        .map(value)
        .collect::<Outcome<_>>()?;

    let directory = &mut organization.directory;
    for row in rows(case, "people.csv")?.unwrap_or_default() {
        directory.people.insert(
            value(&cell(&row, "person")?)?,
            Person {
                tags: tags(&cell(&row, "tags")?)?,
                rules: Vec::new(),
            },
        );
    }
    for row in rows(case, "projects.csv")?.unwrap_or_default() {
        let pattern = cell(&row, "ticket_key_pattern")?;
        directory.projects.insert(
            value(&cell(&row, "project")?)?,
            Project {
                tags: tags(&cell(&row, "tags")?)?,
                ticket_key_pattern: optional(&pattern)
                    .map(|pattern| TicketKeyPattern::new(pattern).map_err(|e| e.to_string()))
                    .transpose()?,
                repositories: list(&cell(&row, "repositories")?)
                    .map(str::to_owned)
                    .collect(),
            },
        );
    }
    for row in rows(case, "tickets.csv")?.unwrap_or_default() {
        directory.tickets.insert(
            value(&cell(&row, "ticket")?)?,
            Ticket {
                project: optional(&cell(&row, "project")?).map(value).transpose()?,
                parent: optional(&cell(&row, "parent")?).map(value).transpose()?,
                title: optional(&cell(&row, "title")?).map(str::to_owned),
                tags: tags(&cell(&row, "tags")?)?,
            },
        );
    }
    for row in rows(case, "rules.csv")?.unwrap_or_default() {
        let rule = rule(&row)?;
        match cell(&row, "owner")?.as_str() {
            "organization" => organization.rules.push(rule),
            person => organization
                .directory
                .people
                .get_mut(&value(person)?)
                .ok_or_else(|| format!("rule {} belongs to unknown person {person}", rule.name))?
                .rules
                .push(rule),
        }
    }
    Ok(organization)
}

/// A rule from `owner,name,when,then`. `when` holds conditions such as
/// `branch starts_with review/`, separated by `;`. `then` is
/// `set name=value;...`, `ticket_key_in SOURCE` or `repository_project`.
fn rule(row: &Row) -> Outcome<Rule> {
    let when = list(&cell(row, "when")?)
        .map(|condition| {
            let mut words = condition.splitn(3, ' ');
            let signal = words.next().unwrap_or_default();
            let test = words.next().unwrap_or_default();
            let test = match words.next() {
                Some(argument) => {
                    let mut test_with_argument = serde_json::Map::new();
                    test_with_argument.insert(test.to_owned(), argument.into());
                    serde_json::Value::Object(test_with_argument)
                }
                None => serde_json::json!(test),
            };
            Ok(serde_json::json!({ "signal": signal, "test": test }))
        })
        .collect::<Outcome<Vec<_>>>()?;
    let then = cell(row, "then")?;
    let then = match then.split_once(' ') {
        Some(("set", pairs)) => serde_json::json!({ "set_tags": tags(pairs)? }),
        Some(("ticket_key_in", source)) => serde_json::json!({ "ticket_key_in": source }),
        _ => serde_json::json!(then),
    };
    serde_json::from_value(serde_json::json!({
        "name": cell(row, "name")?,
        "when": when,
        "then": then,
    }))
    .map_err(|error| format!("rule {row:?}: {error}"))
}

/// The price table of the version the period closes with: every row of
/// `prices.csv` up to that version, later rows replacing earlier ones. Prices
/// are in USD, as published.
fn price_table(case: &Path, version: &str) -> Outcome<PriceTable> {
    let mut by_version: BTreeMap<u32, Vec<Row>> = BTreeMap::new();
    for row in rows(case, "prices.csv")?.unwrap_or_default() {
        let number: u32 = cell(&row, "version")?
            .parse()
            .map_err(|error| format!("version: {error}"))?;
        by_version.entry(number).or_default().push(row);
    }
    let mut table = PriceTable::new(Currency::USD);
    let mut history: Option<History<PriceTable>> = None;
    for rows in by_version.values() {
        for row in rows {
            let price = |kind: &str| -> Outcome<Money> {
                Money::parse(&cell(row, kind)?, Currency::USD).map_err(|error| error.to_string())
            };
            table
                .insert(
                    model(&cell(row, "model")?)?,
                    ModelPrice {
                        input: price("input")?,
                        output: price("output")?,
                        cache_write: price("cache_write")?,
                        cache_read: price("cache_read")?,
                    },
                )
                .map_err(|error| error.to_string())?;
        }
        match &mut history {
            Some(history) => {
                history.record(table.clone());
            }
            None => history = Some(History::new(table.clone())),
        }
    }
    let Some(history) = history else {
        return Ok(table);
    };
    match version {
        "" => Ok(history.current().1.clone()),
        number => {
            let number: u32 = number
                .parse()
                .map_err(|error| format!("price_version: {error}"))?;
            Version::new(number)
                .and_then(|version| history.get(version))
                .cloned()
                .ok_or_else(|| format!("no price table version {number}"))
        }
    }
}

/// The token entries with the bill each is charged on, from
/// `token_entries.csv` and `messages.csv`.
fn token_entries(case: &Path, currency: Currency) -> Outcome<Vec<(String, String, TokenEntry)>> {
    let mut messages: BTreeMap<String, Vec<Row>> = BTreeMap::new();
    for row in rows(case, "messages.csv")?.unwrap_or_default() {
        messages.entry(cell(&row, "entry")?).or_default().push(row);
    }
    let mut entries = Vec::new();
    for row in rows(case, "token_entries.csv")?.ok_or("token_entries.csv is missing")? {
        let id = cell(&row, "entry")?;
        let started_at = moment(&cell(&row, "started_at")?)?;
        let session =
            SessionId::new(&format!("session-{id}")).map_err(|error| error.to_string())?;
        let signals = signals(&cell(&row, "signals")?)?;
        let own = messages.remove(&id).unwrap_or_default();
        let (totals, calls): (Vec<&Row>, Vec<&Row>) = own
            .iter()
            .partition(|row| row.get("message").is_none_or(String::is_empty));
        let segments = if calls.is_empty() {
            let usage = totals
                .into_iter()
                .map(|row| usage(row, currency))
                .collect::<Outcome<Vec<_>>>()?;
            if usage.is_empty() {
                Vec::new()
            } else {
                vec![Segment {
                    signals: signals.clone(),
                    usage,
                }]
            }
        } else if totals.is_empty() {
            let calls = calls
                .into_iter()
                .map(|row| {
                    let message_signals = message_signals(&cell(row, "signals")?)?;
                    Ok(Message {
                        session_id: session.clone(),
                        message_id: MessageId::new(&cell(row, "message")?)
                            .map_err(|error| error.to_string())?,
                        request_id: None,
                        at: moment(&cell(row, "at")?)?,
                        usage: usage(row, currency)?,
                        signals: message_signals,
                    })
                })
                .collect::<Outcome<Vec<_>>>()?;
            TokenEntry::segments_from_messages(&signals, calls)
        } else {
            return Err(format!("entry {id} has both messages and totals"));
        };
        let ended_at = own
            .iter()
            .filter_map(|row| optional(row.get("at").map_or("", String::as_str)))
            .map(moment)
            .collect::<Outcome<Vec<_>>>()?
            .into_iter()
            .max()
            .unwrap_or(started_at);
        let entry = TokenEntry {
            agent_harness: cell(&row, "agent_harness")?,
            agent_name: optional(&cell(&row, "agent_name")?).map(str::to_owned),
            agent_manager: optional(&cell(&row, "person")?).map(value).transpose()?,
            session_id: Some(session),
            started_at,
            ended_at,
            summary: optional(&cell(&row, "summary")?).map(str::to_owned),
            arrival: word(&cell(&row, "arrival")?)?,
            status: word(&cell(&row, "status")?)?,
            tags: tags(&cell(&row, "tags")?)?,
            configured: tags(&cell(&row, "configured")?)?,
            signals,
            segments,
            hand_split: hand_split(&cell(&row, "hand_split")?)?,
        };
        entries.push((id, cell(&row, "bill")?, entry));
    }
    if let Some(entry) = messages.keys().next() {
        return Err(format!("messages.csv names unknown entry {entry}"));
    }
    Ok(entries)
}

/// One message's or one entry's usage.
fn usage(row: &Row, currency: Currency) -> Outcome<Usage> {
    let count = |kind: &str| -> Outcome<u64> {
        match row.get(kind).map_or("", String::as_str) {
            "" => Ok(0),
            text => text.parse().map_err(|error| format!("{kind}: {error}")),
        }
    };
    let money = |column: &str| -> Outcome<Option<Money>> {
        optional(row.get(column).map_or("", String::as_str))
            .map(|text| Money::parse(text, currency).map_err(|error| error.to_string()))
            .transpose()
    };
    Ok(Usage {
        model: model(&cell(row, "model")?)?,
        tokens: TokenCounts {
            input: count("input")?,
            output: count("output")?,
            cache_write: count("cache_write")?,
            cache_read: count("cache_read")?,
        },
        reported_cost: money("reported_cost")?,
        billed_cost: money("billed_cost")?,
    })
}

/// The cost sheet as CSV, with a block per row of `bills.csv`.
fn cost_sheet(
    case: &Path,
    organization: &Organization,
    period: Period,
    settings: &BTreeMap<String, String>,
    splits: &[(String, Status, Period, Vec<Split>)],
) -> Outcome<String> {
    let currency = organization.reporting_currency;
    let mut bills: Vec<Bill> = Vec::new();
    for row in rows(case, "bills.csv")?.unwrap_or_default() {
        let id = cell(&row, "bill")?;
        let bill_currency =
            Currency::new(&cell(&row, "currency")?).map_err(|error| error.to_string())?;
        let amount = |column: &str| -> Outcome<Money> {
            Money::parse(&cell(&row, column)?, bill_currency).map_err(|error| error.to_string())
        };
        bills.push(Bill {
            id,
            provider: cell(&row, "provider")?,
            cost_source: word(&cell(&row, "cost_source")?)?,
            period: Period::parse(&cell(&row, "period")?).map_err(|error| error.to_string())?,
            gross: amount("gross")?,
            discount: amount("discount")?,
            credit: amount("credit")?,
            net: amount("net")?,
            exchange_rate: optional(&cell(&row, "rate")?)
                .map(|rate| {
                    ExchangeRate::new(bill_currency, currency, rate)
                        .map_err(|error| error.to_string())
                })
                .transpose()?,
            revision: match cell(&row, "revision")?.as_str() {
                "" => 1,
                text => text.parse().map_err(|error| format!("revision: {error}"))?,
            },
        });
    }
    if let Some((bill, _, _, _)) = splits
        .iter()
        .find(|(bill, _, _, _)| !bill.is_empty() && !bills.iter().any(|known| known.id == *bill))
    {
        return Err(format!("an entry names bill {bill}, which bills.csv lacks"));
    }
    // Entries on no bill, such as usage on a seat plan, are not on the cost
    // sheet.
    let blocks = bills
        .iter()
        .map(|bill| BlockRequest {
            bill,
            splits: splits
                .iter()
                .filter(|(entry_bill, status, entry_period, _)| {
                    *entry_bill == bill.id
                        && status.counts(organization.approval)
                        && *entry_period == period
                })
                .flat_map(|(_, _, _, splits)| splits)
                .collect(),
        })
        .collect();
    let adjustments = rows(case, "adjustments.csv")?
        .unwrap_or_default()
        .iter()
        .map(|row| {
            Ok(Adjustment {
                period,
                tags: tags(&cell(row, "tags")?)?,
                amount: Money::parse(&cell(row, "amount")?, currency)
                    .map_err(|error| error.to_string())?,
                reason: cell(row, "reason")?,
                posted_by: value(&cell(row, "posted_by")?)?,
                posted_at: moment(&cell(row, "posted_at")?)?,
                late: cell(row, "late")? == "true",
            })
        })
        .collect::<Outcome<Vec<_>>>()?;
    let columns = list(settings.get("columns").map_or("", String::as_str))
        .map(name)
        .collect::<Outcome<Vec<_>>>()?;
    CostSheet::build(&CostSheetRequest {
        period,
        reporting_currency: currency,
        tag_map: &organization.tag_map,
        columns: &columns,
        blocks,
        adjustments: &adjustments,
    })
    .map_err(|error| error.to_string())?
    .to_csv()
    .map_err(|error| error.to_string())
}

/// `reports.csv`: for each filter and grouping it names, in order, what the
/// splits come to by value, with an empty value for splits without the tag.
/// An empty grouping gives the filter's total.
fn reports(case: &Path, items: &[(&Tags, Money)], currency: Currency) -> Outcome<Option<String>> {
    let Some(expected) = rows(case, "reports.csv")? else {
        return Ok(None);
    };
    let mut asked: Vec<(String, String)> = Vec::new();
    for row in expected {
        let key = (cell(&row, "filter")?, cell(&row, "group_by")?);
        if !asked.contains(&key) {
            asked.push(key);
        }
    }
    let mut text = String::from("filter,group_by,value,amount\n");
    for (filter, by) in asked {
        let tag_filter = Filter(tags(&filter)?);
        let groups = match by.as_str() {
            "" => vec![(
                None,
                actual(items.iter().copied(), &tag_filter, currency)
                    .map_err(|error| error.to_string())?,
            )],
            by => group_by(items.iter().copied(), &tag_filter, &name(by)?, currency)
                .map_err(|error| error.to_string())?,
        };
        for (group, amount) in groups {
            let _ignored = writeln!(
                text,
                "{filter},{by},{},{}",
                group.map(|group| group.to_string()).unwrap_or_default(),
                exact(amount)
            );
        }
    }
    Ok(Some(text))
}

/// `budgets.csv`: each token budget next to what its filter matches.
fn budgets(
    case: &Path,
    items: &[(&Tags, Money)],
    period: Period,
    currency: Currency,
) -> Outcome<Option<String>> {
    let Some(expected) = rows(case, "budgets.csv")? else {
        return Ok(None);
    };
    let mut text = String::from("filter,budget,actual,difference,outcome\n");
    for row in expected {
        let filter = cell(&row, "filter")?;
        let budget = TokenBudget {
            filter: Filter(tags(&filter)?),
            period,
            amount: Money::parse(&cell(&row, "budget")?, currency)
                .map_err(|error| error.to_string())?,
        };
        let compared = budget
            .compare(items.iter().copied())
            .map_err(|error| error.to_string())?;
        let outcome = serde_json::to_value(compared.outcome).map_err(|error| error.to_string())?;
        let _ignored = writeln!(
            text,
            "{filter},{},{},{},{}",
            exact(compared.budget),
            exact(compared.actual),
            exact(compared.difference),
            outcome.as_str().unwrap_or_default()
        );
    }
    Ok(Some(text))
}

/// The rows of a CSV file in the case, or `None` if it has no such file.
fn rows(case: &Path, file: &str) -> Outcome<Option<Vec<Row>>> {
    let path = case.join(file);
    if !path.exists() {
        return Ok(None);
    }
    let mut reader = csv::ReaderBuilder::new()
        .trim(csv::Trim::All)
        .from_path(&path)
        .map_err(|error| format!("{file}: {error}"))?;
    reader
        .deserialize()
        .map(|row| row.map_err(|error| format!("{file}: {error}")))
        .collect::<Outcome<Vec<Row>>>()
        .map(Some)
}

fn cell(row: &Row, column: &str) -> Outcome<String> {
    row.get(column)
        .cloned()
        .ok_or_else(|| format!("no column {column} in {row:?}"))
}

fn optional(text: &str) -> Option<&str> {
    Some(text).filter(|text| !text.is_empty())
}

/// The items of a `;`-separated list.
fn list(text: &str) -> impl Iterator<Item = &str> {
    text.split(';')
        .map(str::trim)
        .filter(|item| !item.is_empty())
}

fn name(text: &str) -> Outcome<TagName> {
    TagName::new(text).map_err(|error| error.to_string())
}

fn value(text: &str) -> Outcome<TagValue> {
    TagValue::new(text).map_err(|error| error.to_string())
}

/// Tags written as `name=value` pairs separated by `;`.
fn tags(text: &str) -> Outcome<Tags> {
    Tags::parse(list(text)).map_err(|error| error.to_string())
}

/// Tags written the way `tags` reads them, in tag name order.
fn tag_list(tags: &Tags) -> String {
    tags.iter()
        .map(|(name, value)| format!("{name}={value}"))
        .collect::<Vec<_>>()
        .join(";")
}

/// A model written as `provider/model`.
fn model(text: &str) -> Outcome<ModelRef> {
    let (provider, model) = text
        .split_once('/')
        .ok_or_else(|| format!("model {text} is not provider/model"))?;
    Ok(ModelRef::new(provider, model))
}

fn moment(text: &str) -> Outcome<OffsetDateTime> {
    OffsetDateTime::parse(text, &Rfc3339).map_err(|error| format!("{text}: {error}"))
}

/// An enum value written the way its JSON spells it, such as `submitted`.
fn word<T: DeserializeOwned>(text: &str) -> Outcome<T> {
    serde_json::from_value(serde_json::Value::String(text.to_owned()))
        .map_err(|error| format!("{text}: {error}"))
}

/// A session's signals, written as `name=value` pairs separated by `;`.
/// Ticket key lists are separated by spaces.
fn signals(text: &str) -> Outcome<Signals> {
    let mut signals = Signals::default();
    for (key, value) in pairs(text)? {
        let keys = || value.split(' ').map(str::to_owned).collect::<Vec<_>>();
        match key {
            "branch" => signals.branch = Some(value.to_owned()),
            "working_directory" => signals.working_directory = Some(value.to_owned()),
            "repository" => signals.repository = Some(value.to_owned()),
            "pull_request" => signals.pull_request = Some(number(value)?),
            "subagent_type" => signals.subagent_type = Some(value.to_owned()),
            "prompt_keys" => signals.prompt_ticket_keys = keys(),
            "session_title_keys" => signals.session_title_ticket_keys = keys(),
            "pull_request_title_keys" => signals.pull_request_title_ticket_keys = keys(),
            other => return Err(format!("unknown signal {other}")),
        }
    }
    Ok(signals)
}

/// One message's signals, written like a session's.
fn message_signals(text: &str) -> Outcome<MessageSignals> {
    let mut signals = MessageSignals::default();
    for (key, value) in pairs(text)? {
        match key {
            "branch" => signals.branch = Some(value.to_owned()),
            "working_directory" => signals.working_directory = Some(value.to_owned()),
            "repository" => signals.repository = Some(value.to_owned()),
            "pull_request" => signals.pull_request = Some(number(value)?),
            "subagent_type" => signals.subagent_type = Some(value.to_owned()),
            "prompt_keys" => {
                signals.prompt_ticket_keys = value.split(' ').map(str::to_owned).collect();
            }
            other => return Err(format!("unknown message signal {other}")),
        }
    }
    Ok(signals)
}

fn pairs(text: &str) -> Outcome<Vec<(&str, &str)>> {
    list(text)
        .map(|pair| {
            pair.split_once('=')
                .map(|(key, value)| (key.trim(), value.trim()))
                .ok_or_else(|| format!("{pair} is not name=value"))
        })
        .collect()
}

fn number(text: &str) -> Outcome<u64> {
    text.parse().map_err(|error| format!("{text}: {error}"))
}

/// A split by hand, such as `60 ticket=ACME-50|40 ticket=GLOBEX-7`.
fn hand_split(text: &str) -> Outcome<Vec<HandShare>> {
    text.split('|')
        .map(str::trim)
        .filter(|share| !share.is_empty())
        .map(|share| {
            let (weight, share_tags) = share.split_once(' ').unwrap_or((share, ""));
            Ok(HandShare {
                weight: number(weight)?,
                tags: tags(share_tags)?,
            })
        })
        .collect()
}

/// An amount with every digit it has, at least to the cent, such as
/// `52.98` or `0.0015`.
fn exact(money: Money) -> String {
    let nanos = money.nanos();
    let sign = if nanos.is_negative() { "-" } else { "" };
    let magnitude = nanos.unsigned_abs();
    let units = magnitude.checked_div(1_000_000_000).unwrap_or_default();
    let fraction = format!(
        "{:09}",
        magnitude.checked_rem(1_000_000_000).unwrap_or_default()
    );
    let digits = fraction.trim_end_matches('0');
    let digits = if digits.len() < 2 {
        fraction.get(..2).unwrap_or_default()
    } else {
        digits
    };
    format!("{sign}{units}.{digits}")
}
