//! `/usage` and `/billing` in the interactive session: the ModelGateway
//! account, usage and billing replies (`crate::gateway`) as transcript text,
//! and the billing actions (adding credit and buying a plan open a checkout
//! page in the browser; a plan paid from the balance needs `/billing confirm`).

use crate::gateway::{self, GatewayError};
use chrono::{Local, TimeZone};
use ctx_core::paths::DataLayout;
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::BTreeMap;

const DEFAULT_QUOTA_PER_UNIT: f64 = 500_000.0;
/// Rows shown per breakdown (by model, by project).
const TOP_ROWS: usize = 8;
/// Bars in the daily-cost sparkline; longer periods fold days together.
const SPARK_WIDTH: usize = 60;

/// What `/usage` or `/billing …` asks for.
#[derive(Debug, Clone, PartialEq)]
pub enum Request {
    Usage(UsageQuery),
    Billing,
    TopUp(i64),
    Plan { id: i64, from_balance: bool, confirmed: bool },
    Prefer(&'static str),
}

/// The task's answer: text for the transcript, or a plan bought from the
/// balance waiting for `/billing confirm`.
#[derive(Debug)]
pub enum Reply {
    Text(String),
    ConfirmPlan { id: i64, text: String },
}

/// What `/usage` shows: a period, optionally narrowed to some models or
/// projects.
#[derive(Debug, Clone, PartialEq)]
pub struct UsageQuery {
    pub period: Period,
    /// Lowercase parts of model names; a row matches when its model contains
    /// any of them. Empty for every model.
    pub models: Vec<String>,
    /// The same for project names (`(unattributed)` for rows without one).
    pub projects: Vec<String>,
}

/// A `/usage` period.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Period {
    /// Since local midnight.
    Today,
    Hours(u32),
    Days(u32),
    /// Everything the gateway returns (it keeps at most 400 days).
    All,
}

impl Period {
    fn label(self) -> String {
        let plural = |n: u32| if n == 1 { "" } else { "s" };
        match self {
            Period::Today => "today".into(),
            Period::Hours(1) => "last hour".into(),
            Period::Hours(h) => format!("last {h} hour{}", plural(h)),
            Period::Days(d) => format!("last {d} day{}", plural(d)),
            Period::All => "all time".into(),
        }
    }

    /// The report's start, or `None` to let the gateway pick its earliest.
    fn start(self, now: i64) -> Option<i64> {
        match self {
            Period::Today => Some(start_of_today()),
            Period::Hours(h) => Some(now - i64::from(h) * 3_600),
            Period::Days(d) => Some(now - i64::from(d) * 86_400),
            Period::All => None,
        }
    }
}

impl UsageQuery {
    fn label(&self) -> String {
        let mut label = self.period.label();
        for (key, values) in [("model", &self.models), ("project", &self.projects)] {
            if !values.is_empty() {
                label.push_str(&format!(" · {key} {}", values.join(", ")));
            }
        }
        label
    }

    fn is_filtered(&self) -> bool {
        !self.models.is_empty() || !self.projects.is_empty()
    }

    fn matches(&self, row: &UsageRow) -> bool {
        let any = |wanted: &[String], name: &str| wanted.is_empty() || wanted.iter().any(|w| name.to_lowercase().contains(w.as_str()));
        any(&self.models, &row.model_title()) && any(&self.projects, &row.project_title())
    }
}

/// `/usage` periods, for completion: name, about.
pub const PERIODS: &[(&str, &str)] = &[
    ("today", "since midnight"),
    ("24h", "the last 24 hours"),
    ("7d", "the last 7 days"),
    ("30d", "the last 30 days (default)"),
    ("90d", "the last 90 days"),
    ("year", "the last 365 days"),
    ("all", "everything the gateway keeps (up to 400 days)"),
];

/// `/usage` filters, for completion: key, about.
pub const FILTERS: &[(&str, &str)] = &[
    ("model:", "only models whose name contains this (a,b for several)"),
    ("project:", "only these projects; project:unattributed for requests without one"),
];

/// `/billing` subcommands, for completion and help.
pub const BILLING_COMMANDS: &[(&str, &str)] = &[
    ("topup", "<amount>: add credit (opens checkout in the browser)"),
    ("plan", "<id> [balance]: buy a plan by card, or from the balance"),
    ("prefer", "<order>: plan-first, balance-first, plan-only or balance-only"),
    ("confirm", "buy the plan from the balance that /billing plan … balance priced"),
];

/// How requests draw on a plan and the balance: short name, wire name, about.
pub const PREFERENCES: &[(&str, &str, &str)] = &[
    ("plan-first", "subscription_first", "plan first, then balance"),
    ("balance-first", "wallet_first", "balance first, then plan"),
    ("plan-only", "subscription_only", "plan only"),
    ("balance-only", "wallet_only", "balance only"),
];

/// `/usage [period] [model:<name>[,<name>…]] [project:<name>[,<name>…]]`,
/// in any order; the period defaults to 30 days.
pub fn parse_usage(arg: &str) -> Result<Request, String> {
    let mut query = UsageQuery { period: Period::Days(30), models: Vec::new(), projects: Vec::new() };
    let mut period = None;
    for word in arg.split_whitespace() {
        let lower = word.to_lowercase();
        if let Some((key, value)) = lower.split_once([':', '=']) {
            let list = match key {
                "model" | "models" => &mut query.models,
                "project" | "projects" => &mut query.projects,
                _ => return Err(format!("unknown filter `{word}`; use model:<name> or project:<name>")),
            };
            let before = list.len();
            list.extend(value.split(',').map(str::trim).filter(|v| !v.is_empty()).map(String::from));
            if list.len() == before {
                return Err(format!("`{word}` names no {key}; e.g. /usage {key}:{}", if key.starts_with("model") { "gpt" } else { "my-app" }));
            }
        } else {
            let parsed = parse_period(&lower).ok_or_else(|| format!("unknown period `{word}`; use today, 24h, 7d, 30d, 90d, year, all, or a number of hours (up to 168h) or days (up to 371d)"))?;
            if period.replace(parsed).is_some() {
                return Err("give one period, e.g. /usage 7d model:gpt".into());
            }
        }
    }
    if let Some(period) = period {
        query.period = period;
    }
    Ok(Request::Usage(query))
}

fn parse_period(t: &str) -> Option<Period> {
    match t {
        "today" | "day" => Some(Period::Today),
        "all" | "ever" => Some(Period::All),
        "week" => Some(Period::Days(7)),
        "month" => Some(Period::Days(30)),
        "year" => Some(Period::Days(365)),
        other => match other.strip_suffix('h') {
            Some(hours) => hours.parse::<u32>().ok().filter(|h| (1..=168).contains(h)).map(Period::Hours),
            None => other.strip_suffix('d').unwrap_or(other).parse::<u32>().ok().filter(|d| (1..=371).contains(d)).map(Period::Days),
        },
    }
}

pub fn parse_billing(arg: &str) -> Result<Request, String> {
    let words: Vec<&str> = arg.split_whitespace().collect();
    match words.as_slice() {
        [] => Ok(Request::Billing),
        ["topup" | "add", amount] => amount.trim_start_matches('$').parse::<i64>().ok().filter(|a| *a > 0).map(Request::TopUp).ok_or_else(|| format!("`{amount}` is not a whole amount; e.g. /billing topup 20")),
        ["topup" | "add"] => Err("/billing topup <amount>, e.g. /billing topup 20".into()),
        ["plan" | "subscribe", id, rest @ ..] => {
            let id = id.trim_start_matches('#').parse::<i64>().map_err(|_| format!("`{id}` is not a plan id; /billing lists them"))?;
            match rest {
                [] | ["card"] => Ok(Request::Plan { id, from_balance: false, confirmed: false }),
                ["balance"] => Ok(Request::Plan { id, from_balance: true, confirmed: false }),
                _ => Err("/billing plan <id> [balance]".into()),
            }
        }
        ["plan" | "subscribe"] => Err("/billing plan <id> [balance]; /billing lists the plans".into()),
        ["prefer", order] => {
            let t = order.to_ascii_lowercase();
            PREFERENCES.iter().find(|(short, wire, _)| *short == t || *wire == t).map(|(_, wire, _)| Request::Prefer(wire)).ok_or_else(|| format!("unknown order `{order}`; use plan-first, balance-first, plan-only or balance-only"))
        }
        ["prefer"] => Err("/billing prefer plan-first|balance-first|plan-only|balance-only".into()),
        _ => Err(format!("unknown /billing command `{arg}`; use topup, plan, prefer or confirm")),
    }
}

/// Runs `request` against the stored credential.
pub async fn run(layout: DataLayout, request: Request) -> Result<Reply, String> {
    let text = |e: GatewayError| match e {
        GatewayError::SignedOut => "not signed in to ModelGateway; /signin first".to_string(),
        GatewayError::Unauthorized { message, .. } => format!("{message}; /signout and /signin again"),
        other => other.to_string(),
    };
    match request {
        Request::Usage(query) => {
            let account: Account = parse(gateway::account(&layout, None).await.map_err(text)?)?;
            let end = Local::now().timestamp();
            let report: UsageReport = parse(gateway::usage(&layout, None, query.period.start(end), Some(end)).await.map_err(text)?)?;
            // "all": the chart starts at the first recorded hour, not 400 days back.
            let start = query.period.start(end).or_else(|| report.rows.iter().map(|r| r.created_at).min()).unwrap_or(report.start_timestamp.min(end));
            Ok(Reply::Text(format!("{}\n\n{}", account_text(&account), usage_text(&report, &query, start, end))))
        }
        Request::Billing => {
            let billing: Billing = parse(gateway::billing(&layout, None).await.map_err(text)?)?;
            Ok(Reply::Text(billing_text(&billing)))
        }
        Request::TopUp(amount) => {
            let billing: Billing = parse(gateway::billing(&layout, None).await.map_err(text)?)?;
            let wallet = billing.wallet_url.clone().filter(|u| !u.is_empty());
            if !billing.topup.enabled || !billing.topup.in_app {
                let url = wallet.ok_or("adding credit is not available from here; use the gateway's wallet page")?;
                crate::tui::open_in_browser(&url);
                return Ok(Reply::Text(format!("adding credit happens on the wallet page; opened it in your browser\n  {url}")));
            }
            if amount < billing.topup.min_amount || amount > billing.topup.max_amount {
                return Err(format!("the amount must be between {} and {}", billing.topup.label(billing.topup.min_amount), billing.topup.label(billing.topup.max_amount)));
            }
            let quote: Quote = parse(gateway::billing_post(&layout, None, "quote", json!({"amount": amount})).await.map_err(text)?).unwrap_or_default();
            let checkout: Checkout = parse(gateway::billing_post(&layout, None, "topup", json!({"amount": amount})).await.map_err(text)?)?;
            let url = checkout.checkout_url.filter(|u| !u.is_empty()).ok_or("the gateway sent no checkout link")?;
            crate::tui::open_in_browser(&url);
            let price = if quote.pay_money > 0.0 { format!(" for {}", money(quote.pay_money, quote.currency.as_deref())) } else { String::new() };
            Ok(Reply::Text(format!("opened checkout in your browser: {} of credit{price}\n  {url}\nthe balance updates once the payment goes through; /usage shows it", billing.topup.label(amount))))
        }
        Request::Plan { id, from_balance, confirmed } => {
            let billing: Billing = parse(gateway::billing(&layout, None).await.map_err(text)?)?;
            let plan = billing.plans.iter().find(|p| p.id == id).ok_or_else(|| format!("no plan #{id} on sale; /billing lists them"))?;
            if from_balance && !plan.balance {
                return Err(format!("{} cannot be paid from the balance; /billing plan {id} buys it by card", plan.name()));
            }
            if !from_balance && !plan.stripe {
                return Err(format!("{} cannot be paid by card; /billing plan {id} balance pays from the balance", plan.name()));
            }
            if from_balance && !confirmed {
                let account: Account = parse(gateway::account(&layout, None).await.map_err(text)?)?;
                return Ok(Reply::ConfirmPlan {
                    id,
                    text: format!(
                        "{} costs {} ({}), paid from your balance ({} now)\n/billing confirm buys it; any other /billing command cancels",
                        plan.name(),
                        plan.price_title(),
                        plan.allowance_title(billing.quota_per_unit),
                        account.format_quota(account.quota)
                    ),
                });
            }
            let method = if from_balance { "balance" } else { "stripe" };
            let checkout: Checkout = parse(gateway::billing_post(&layout, None, "subscribe", json!({"plan_id": id, "method": method})).await.map_err(text)?)?;
            match checkout.checkout_url.filter(|u| !u.is_empty()) {
                Some(url) => {
                    crate::tui::open_in_browser(&url);
                    Ok(Reply::Text(format!("opened checkout for {} ({}) in your browser\n  {url}", plan.name(), plan.price_title())))
                }
                None if checkout.subscribed == Some(true) => Ok(Reply::Text(format!("bought {} ({}) from your balance; /usage shows the plan", plan.name(), plan.price_title()))),
                None => Err("the gateway neither sent a checkout link nor confirmed the plan".into()),
            }
        }
        Request::Prefer(wire) => {
            let reply = gateway::billing_preference(&layout, None, json!({"billing_preference": wire})).await.map_err(text)?;
            let now = reply.get("billing_preference").and_then(Value::as_str).unwrap_or(wire);
            Ok(Reply::Text(format!("requests now draw on: {}", preference_title(now))))
        }
    }
}

fn parse<T: serde::de::DeserializeOwned>(value: Value) -> Result<T, String> {
    serde_json::from_value(value).map_err(|e| format!("unexpected reply from the gateway ({e})"))
}

fn start_of_today() -> i64 {
    let today = Local::now().date_naive().and_hms_opt(0, 0, 0).unwrap_or_default();
    Local.from_local_datetime(&today).earliest().map(|t| t.timestamp()).unwrap_or_else(|| Local::now().timestamp() - 86_400)
}

// ---------------------------------------------------------------------------
// Wire shapes (the desktop app's `Models/ModelGateway.swift`)
// ---------------------------------------------------------------------------

fn per_unit() -> f64 {
    DEFAULT_QUOTA_PER_UNIT
}

fn yes() -> bool {
    true
}

#[derive(Debug, Deserialize)]
struct Account {
    #[serde(default)]
    username: String,
    #[serde(default)]
    display_name: String,
    #[serde(default)]
    quota: i64,
    #[serde(default)]
    used_quota: i64,
    #[serde(default)]
    request_count: i64,
    #[serde(default = "per_unit")]
    quota_per_unit: f64,
    #[serde(default = "yes")]
    display_in_currency: bool,
    #[serde(default)]
    billing_preference: Option<String>,
    #[serde(default)]
    subscriptions: Option<Vec<Subscription>>,
}

impl Account {
    fn format_quota(&self, amount: i64) -> String {
        if self.display_in_currency { usd(amount as f64 / self.quota_per_unit.max(1.0)) } else { format!("{} units", compact(amount)) }
    }
}

#[derive(Debug, Deserialize)]
struct Subscription {
    #[serde(default)]
    plan_id: i64,
    #[serde(default)]
    title: String,
    #[serde(default)]
    quota_reset_period: String,
    #[serde(default)]
    amount_total: i64,
    #[serde(default)]
    amount_used: i64,
    #[serde(default)]
    amount_reserved: i64,
    #[serde(default)]
    end_time: i64,
    #[serde(default)]
    next_reset_time: i64,
}

#[derive(Debug, Deserialize)]
struct UsageReport {
    #[serde(default)]
    start_timestamp: i64,
    #[serde(default = "per_unit")]
    quota_per_unit: f64,
    #[serde(default)]
    rows: Vec<UsageRow>,
}

#[derive(Debug, Deserialize)]
struct UsageRow {
    #[serde(default)]
    created_at: i64,
    #[serde(default)]
    model_name: String,
    #[serde(default)]
    project: String,
    #[serde(default)]
    count: i64,
    #[serde(default)]
    quota: i64,
    #[serde(default)]
    token_used: i64,
}

impl UsageRow {
    fn model_title(&self) -> String {
        if self.model_name.is_empty() { "(unknown model)".into() } else { self.model_name.clone() }
    }

    fn project_title(&self) -> String {
        if self.project.is_empty() { "(unattributed)".into() } else { self.project.clone() }
    }
}

#[derive(Debug, Deserialize)]
struct Billing {
    #[serde(default)]
    billing_preference: String,
    #[serde(default = "per_unit")]
    quota_per_unit: f64,
    #[serde(default)]
    wallet_url: Option<String>,
    #[serde(default)]
    topup: TopUp,
    #[serde(default)]
    plans: Vec<Plan>,
    #[serde(default)]
    has_active_subscription: bool,
}

#[derive(Debug, Deserialize)]
#[serde(default)]
struct TopUp {
    enabled: bool,
    in_app: bool,
    min_amount: i64,
    max_amount: i64,
    amount_options: Vec<i64>,
    /// Amount (as a string) → price multiplier.
    discounts: BTreeMap<String, f64>,
    amount_unit: String,
}

impl Default for TopUp {
    fn default() -> Self {
        Self { enabled: false, in_app: false, min_amount: 1, max_amount: 10_000, amount_options: Vec::new(), discounts: BTreeMap::new(), amount_unit: "usd".into() }
    }
}

impl TopUp {
    fn label(&self, amount: i64) -> String {
        if self.amount_unit == "quota" { format!("{} units", group_digits(amount)) } else { usd(amount as f64) }
    }

    fn discount(&self, amount: i64) -> Option<String> {
        let multiplier = *self.discounts.get(&amount.to_string())?;
        (multiplier > 0.0 && multiplier < 1.0).then(|| format!("{}% off", ((1.0 - multiplier) * 100.0).round() as i64))
    }
}

#[derive(Debug, Deserialize)]
struct Plan {
    #[serde(default)]
    id: i64,
    #[serde(default)]
    title: String,
    #[serde(default)]
    subtitle: String,
    #[serde(default)]
    price_amount: f64,
    #[serde(default)]
    currency: String,
    #[serde(default)]
    duration_unit: String,
    #[serde(default)]
    duration_value: i64,
    #[serde(default)]
    quota_reset_period: String,
    #[serde(default)]
    total_amount: i64,
    #[serde(default)]
    stripe: bool,
    #[serde(default)]
    balance: bool,
}

impl Plan {
    fn name(&self) -> String {
        if self.title.trim().is_empty() { format!("Plan #{}", self.id) } else { self.title.trim().to_string() }
    }

    /// "$20 / month", "$50 / 3 months", "Free".
    fn price_title(&self) -> String {
        if self.price_amount <= 0.0 {
            return "Free".into();
        }
        let price = money(self.price_amount, Some(&self.currency));
        if !["year", "month", "day", "hour"].contains(&self.duration_unit.as_str()) || self.duration_value <= 0 {
            return price;
        }
        if self.duration_value == 1 { format!("{price} / {}", self.duration_unit) } else { format!("{price} / {} {}s", self.duration_value, self.duration_unit) }
    }

    /// "$10 of usage, resets monthly" / "unlimited usage".
    fn allowance_title(&self, quota_per_unit: f64) -> String {
        let reset = reset_suffix(&self.quota_reset_period);
        if self.total_amount <= 0 { format!("unlimited usage{reset}") } else { format!("{} of usage{reset}", usd(self.total_amount as f64 / quota_per_unit.max(1.0))) }
    }
}

#[derive(Debug, Default, Deserialize)]
struct Quote {
    #[serde(default)]
    pay_money: f64,
    #[serde(default)]
    currency: Option<String>,
}

#[derive(Debug, Deserialize)]
struct Checkout {
    #[serde(default)]
    checkout_url: Option<String>,
    #[serde(default)]
    subscribed: Option<bool>,
}

// ---------------------------------------------------------------------------
// Text
// ---------------------------------------------------------------------------

fn account_text(account: &Account) -> String {
    let name = if account.display_name.trim().is_empty() { account.username.clone() } else { format!("{} ({})", account.display_name.trim(), account.username) };
    let mut s = format!(
        "ModelGateway · {name}\n  balance   {}\n  used      {} over {} requests",
        account.format_quota(account.quota),
        account.format_quota(account.used_quota),
        group_digits(account.request_count)
    );
    match &account.subscriptions {
        Some(plans) if !plans.is_empty() => {
            for plan in plans {
                let title = if plan.title.trim().is_empty() { format!("Plan #{}", plan.plan_id) } else { plan.title.trim().to_string() };
                let left = if plan.amount_total <= 0 {
                    "unlimited".to_string()
                } else {
                    let remaining = (plan.amount_total - plan.amount_used - plan.amount_reserved).max(0);
                    format!("{} of {} left", account.format_quota(remaining), account.format_quota(plan.amount_total))
                };
                let mut line = format!("\n  plan      {title} · {left}{}", reset_suffix(&plan.quota_reset_period));
                if plan.next_resets_before_end() {
                    line.push_str(&format!(" (next {})", date(plan.next_reset_time)));
                }
                if plan.end_time > 0 {
                    line.push_str(&format!(" · ends {}", date(plan.end_time)));
                }
                s.push_str(&line);
            }
        }
        Some(_) => s.push_str("\n  plan      none (pay as you go from the balance)"),
        None => {}
    }
    if let Some(preference) = account.billing_preference.as_deref().filter(|p| !p.is_empty()) {
        s.push_str(&format!("\n  draws on  {}", preference_title(preference)));
    }
    s
}

impl Subscription {
    fn next_resets_before_end(&self) -> bool {
        self.next_reset_time > 0 && (self.end_time <= 0 || self.next_reset_time < self.end_time)
    }
}

#[derive(Default, Clone, Copy)]
struct Totals {
    quota: i64,
    tokens: i64,
    requests: i64,
}

impl Totals {
    fn add(&mut self, row: &UsageRow) {
        self.quota += row.quota;
        self.tokens += row.token_used;
        self.requests += row.count;
    }
}

fn usage_text(report: &UsageReport, query: &UsageQuery, start: i64, end: i64) -> String {
    let per_unit = report.quota_per_unit.max(1.0);
    let cost = |quota: i64| usd(quota as f64 / per_unit);
    let rows: Vec<&UsageRow> = report.rows.iter().filter(|r| query.matches(r)).collect();
    let mut total = Totals::default();
    let mut models: BTreeMap<String, Totals> = BTreeMap::new();
    let mut projects: BTreeMap<String, Totals> = BTreeMap::new();
    for row in &rows {
        total.add(row);
        models.entry(row.model_title()).or_default().add(row);
        projects.entry(row.project_title()).or_default().add(row);
    }
    let mut s = format!("{}  {} · {} tokens · {} requests", query.label(), cost(total.quota), compact(total.tokens), group_digits(total.requests));
    if rows.is_empty() {
        s.push_str(if query.is_filtered() && !report.rows.is_empty() { "\n  nothing in this period matches the filter" } else { "\n  no usage in this period" });
        return s;
    }
    if let Some(spark) = sparkline(&rows, start, end) {
        s.push_str(&format!("\n  {spark}"));
    }
    for (title, series) in [("by model", models), ("by project", projects)] {
        let mut rows: Vec<(String, Totals)> = series.into_iter().collect();
        rows.sort_by(|a, b| b.1.quota.cmp(&a.1.quota).then(b.1.requests.cmp(&a.1.requests)).then(a.0.cmp(&b.0)));
        let width = rows.iter().take(TOP_ROWS).map(|(name, _)| name.chars().count()).max().unwrap_or(0).min(36);
        s.push_str(&format!("\n\n{title}"));
        for (name, t) in rows.iter().take(TOP_ROWS) {
            let name: String = name.chars().take(width).collect();
            s.push_str(&format!("\n  {name:<width$}  {:>9}  {:>7} tok  {:>7} req", cost(t.quota), compact(t.tokens), group_digits(t.requests)));
        }
        if rows.len() > TOP_ROWS {
            let rest = rows.iter().skip(TOP_ROWS).fold(Totals::default(), |mut acc, (_, t)| {
                acc.quota += t.quota;
                acc.tokens += t.tokens;
                acc.requests += t.requests;
                acc
            });
            let others = format!("{} more", rows.len() - TOP_ROWS);
            s.push_str(&format!("\n  {others:<width$}  {:>9}  {:>7} tok  {:>7} req", cost(rest.quota), compact(rest.tokens), group_digits(rest.requests)));
        }
    }
    s
}

/// Cost as block characters, oldest first, with the span it covers: per
/// hour for periods of up to two days, per day otherwise; `None` for a
/// single hour or day.
fn sparkline(rows: &[&UsageRow], start: i64, end: i64) -> Option<String> {
    let local = |ts: i64| Local.timestamp_opt(ts, 0).single();
    let day = |ts: i64| local(ts).map(|t| t.date_naive());
    let hourly = end - start <= 2 * 86_400;
    // (slots, the slot of a timestamp, the span label, the slot's unit)
    let (slots, slot, span, unit): (i64, Box<dyn Fn(i64) -> Option<i64>>, String, &str) = if hourly {
        let first = start.div_euclid(3_600);
        let hour = |h: i64| local(h * 3_600).map(|t| t.format("%a %-I%P").to_string()).unwrap_or_default();
        let span = format!("{} → {}", hour(first), hour(end.div_euclid(3_600)));
        (end.div_euclid(3_600) - first + 1, Box::new(move |ts| Some(ts.div_euclid(3_600) - first)), span, "hour")
    } else {
        let (first, last) = (day(start)?, day(end)?);
        let span = format!("{} → {}", first.format("%b %-d"), last.format("%b %-d"));
        ((last - first).num_days() + 1, Box::new(move |ts| day(ts).map(|d| (d - first).num_days())), span, "day")
    };
    if slots < 2 {
        return None;
    }
    let slots = slots as usize;
    let per_bar = slots.div_ceil(SPARK_WIDTH);
    let mut bars = vec![0i64; slots.div_ceil(per_bar)];
    for row in rows {
        if let Some(index) = slot(row.created_at) {
            if index >= 0 && (index as usize) < slots {
                bars[index as usize / per_bar] += row.quota;
            }
        }
    }
    let max = *bars.iter().max()?;
    if max <= 0 {
        return None;
    }
    const BLOCKS: [char; 8] = ['▁', '▂', '▃', '▄', '▅', '▆', '▇', '█'];
    let line: String = bars.iter().map(|&q| if q <= 0 { '·' } else { BLOCKS[((q as f64 / max as f64) * 7.0).round() as usize] }).collect();
    let unit = if per_bar == 1 { unit.to_string() } else { format!("{per_bar} {unit}s") };
    Some(format!("{line}  cost per {unit}, {span}"))
}

fn billing_text(billing: &Billing) -> String {
    let mut s = format!("ModelGateway billing\n  requests draw on  {}", preference_title(&billing.billing_preference));
    s.push_str("\n  /billing prefer plan-first|balance-first|plan-only|balance-only");
    let topup = &billing.topup;
    if topup.enabled && topup.in_app {
        let options: Vec<String> = topup.amount_options.iter().map(|&a| match topup.discount(a) {
            Some(off) => format!("{} ({off})", topup.label(a)),
            None => topup.label(a),
        }).collect();
        s.push_str("\n\nadd credit  /billing topup <amount>");
        if !options.is_empty() {
            s.push_str(&format!("\n  {}", options.join(" · ")));
        }
        s.push_str(&format!("\n  any amount from {} to {}", topup.label(topup.min_amount), topup.label(topup.max_amount)));
    } else if let Some(url) = billing.wallet_url.as_deref().filter(|u| !u.is_empty()) {
        s.push_str(&format!("\n\nadd credit on the wallet page: {url}"));
    }
    if billing.plans.is_empty() {
        s.push_str("\n\nno plans on sale");
    } else {
        s.push_str(&format!("\n\nplans{}  /billing plan <id> [balance]", if billing.has_active_subscription { " (you have an active one; /usage shows it)" } else { "" }));
        for plan in &billing.plans {
            let pay: Vec<&str> = [(plan.stripe, "card"), (plan.balance, "balance")].iter().filter(|(on, _)| *on).map(|(_, how)| *how).collect();
            s.push_str(&format!("\n  #{:<3} {} · {} · {}", plan.id, plan.name(), plan.price_title(), plan.allowance_title(billing.quota_per_unit)));
            if !pay.is_empty() {
                s.push_str(&format!(" · pay by {}", pay.join(" or ")));
            }
            if !plan.subtitle.trim().is_empty() {
                s.push_str(&format!("\n        {}", plan.subtitle.trim()));
            }
        }
    }
    s
}

fn preference_title(wire: &str) -> String {
    PREFERENCES.iter().find(|(_, w, _)| *w == wire).map(|(short, _, about)| format!("{about} ({short})")).unwrap_or_else(|| wire.to_string())
}

fn reset_suffix(period: &str) -> String {
    match period {
        "daily" | "weekly" | "monthly" => format!(", resets {period}"),
        "custom" => ", resets periodically".into(),
        _ => String::new(),
    }
}

fn date(ts: i64) -> String {
    Local.timestamp_opt(ts, 0).single().map(|t| t.format("%b %-d, %Y").to_string()).unwrap_or_default()
}

/// "$12.34"; tiny non-zero amounts show three decimals, zero is "$0.00".
fn usd(value: f64) -> String {
    if value == 0.0 {
        "$0.00".into()
    } else if value.abs() < 0.01 {
        format!("${value:.3}")
    } else if value.abs() >= 1000.0 {
        format!("${}", compact(value.round() as i64))
    } else {
        format!("${value:.2}")
    }
}

/// A price in its currency ("$20", "€9.50").
fn money(value: f64, currency: Option<&str>) -> String {
    let amount = if value.fract() == 0.0 { format!("{value:.0}") } else { format!("{value:.2}") };
    match currency.map(str::to_ascii_uppercase).as_deref() {
        None | Some("") | Some("USD") => format!("${amount}"),
        Some("EUR") => format!("€{amount}"),
        Some("GBP") => format!("£{amount}"),
        Some(other) => format!("{amount} {other}"),
    }
}

/// "12.3k", "1.2M", "845".
fn compact(value: i64) -> String {
    let n = value as f64;
    let (scaled, suffix) = match value.abs() {
        0..=999 => return value.to_string(),
        1_000..=999_999 => (n / 1e3, "k"),
        1_000_000..=999_999_999 => (n / 1e6, "M"),
        _ => (n / 1e9, "B"),
    };
    let text = if scaled.abs() < 10.0 { format!("{scaled:.1}") } else { format!("{scaled:.0}") };
    format!("{}{suffix}", text.strip_suffix(".0").unwrap_or(&text))
}

/// "1,234,567".
fn group_digits(value: i64) -> String {
    let digits = value.unsigned_abs().to_string();
    let mut out = String::new();
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i) % 3 == 0 {
            out.push(',');
        }
        out.push(c);
    }
    if value < 0 { format!("-{out}") } else { out }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn commands_parse() {
        let usage = |period, models: &[&str], projects: &[&str]| Ok(Request::Usage(UsageQuery { period, models: models.iter().map(|s| s.to_string()).collect(), projects: projects.iter().map(|s| s.to_string()).collect() }));
        assert_eq!(parse_usage(""), usage(Period::Days(30), &[], &[]));
        assert_eq!(parse_usage("7d"), usage(Period::Days(7), &[], &[]));
        assert_eq!(parse_usage("24h"), usage(Period::Hours(24), &[], &[]));
        assert_eq!(parse_usage("all"), usage(Period::All, &[], &[]));
        assert_eq!(parse_usage("today"), usage(Period::Today, &[], &[]));
        assert_eq!(parse_usage("model:GPT,claude 7d project=ctx"), usage(Period::Days(7), &["gpt", "claude"], &["ctx"]));
        assert_eq!(parse_usage("project:a project:b"), usage(Period::Days(30), &[], &["a", "b"]));
        assert!(parse_usage("forever").is_err());
        assert!(parse_usage("7d 30d").is_err(), "one period");
        assert!(parse_usage("model:").is_err());
        assert!(parse_usage("team:x").is_err());
        assert!(parse_usage("500h").is_err());
        assert_eq!(parse_billing(""), Ok(Request::Billing));
        assert_eq!(parse_billing("topup $20"), Ok(Request::TopUp(20)));
        assert!(parse_billing("topup -5").is_err());
        assert_eq!(parse_billing("plan #3 balance"), Ok(Request::Plan { id: 3, from_balance: true, confirmed: false }));
        assert_eq!(parse_billing("plan 3"), Ok(Request::Plan { id: 3, from_balance: false, confirmed: false }));
        assert_eq!(parse_billing("prefer balance-first"), Ok(Request::Prefer("wallet_first")));
        assert_eq!(parse_billing("prefer subscription_only"), Ok(Request::Prefer("subscription_only")));
        assert!(parse_billing("refund").is_err());
    }

    #[test]
    fn numbers_format() {
        assert_eq!(usd(0.0), "$0.00");
        assert_eq!(usd(0.004), "$0.004");
        assert_eq!(usd(12.345), "$12.35");
        assert_eq!(compact(1_234), "1.2k");
        assert_eq!(compact(12_000_000), "12M");
        assert_eq!(group_digits(1_234_567), "1,234,567");
        assert_eq!(money(20.0, Some("usd")), "$20");
    }

    #[test]
    fn usage_breaks_down_by_model_and_project() {
        let now = Local::now().timestamp();
        let report: UsageReport = serde_json::from_value(json!({
            "quota_per_unit": 500000,
            "rows": [
                {"created_at": now - 3600, "model_name": "gpt-5", "project": "ctx", "count": 2, "quota": 500000, "token_used": 1500},
                {"created_at": now - 86400 * 2, "model_name": "o3", "project": "", "count": 1, "quota": 250000, "token_used": 500},
            ]
        }))
        .unwrap();
        let query = |period, models: &[&str], projects: &[&str]| UsageQuery { period, models: models.iter().map(|s| s.to_string()).collect(), projects: projects.iter().map(|s| s.to_string()).collect() };
        let text = usage_text(&report, &query(Period::Days(7), &[], &[]), now - 7 * 86_400, now);
        assert!(text.starts_with("last 7 days  $1.50 · 2k tokens · 3 requests"), "{text}");
        assert!(text.contains("cost per day"));
        let by_model = text.split("by model").nth(1).unwrap();
        assert!(by_model.find("gpt-5").unwrap() < by_model.find("o3").unwrap(), "biggest spender first");
        assert!(text.contains("(unattributed)"));

        let text = usage_text(&report, &query(Period::Days(7), &["gpt"], &[]), now - 7 * 86_400, now);
        assert!(text.starts_with("last 7 days · model gpt  $1.00 · 1.5k tokens · 2 requests"), "{text}");
        assert!(!text.contains("o3"), "{text}");
        let text = usage_text(&report, &query(Period::Days(7), &[], &["unattributed"]), now - 7 * 86_400, now);
        assert!(text.contains("$0.50 · 500 tokens · 1 requests") && !text.contains("gpt-5"), "{text}");
        let text = usage_text(&report, &query(Period::Days(7), &["claude"], &[]), now - 7 * 86_400, now);
        assert!(text.contains("nothing in this period matches the filter"), "{text}");
        let text = usage_text(&report, &query(Period::Hours(24), &[], &[]), now - 86_400, now);
        assert!(text.contains("cost per hour"), "{text}");
    }

    #[test]
    fn billing_lists_plans_and_top_ups() {
        let billing: Billing = serde_json::from_value(json!({
            "billing_preference": "subscription_first",
            "topup": {"enabled": true, "in_app": true, "amount_options": [10, 20], "discounts": {"20": 0.95}},
            "plans": [{"id": 3, "title": "Pro", "price_amount": 20, "currency": "USD", "duration_unit": "month", "duration_value": 1, "quota_reset_period": "monthly", "total_amount": 5000000, "stripe": true, "balance": true}]
        }))
        .unwrap();
        let text = billing_text(&billing);
        assert!(text.contains("plan first, then balance (plan-first)"));
        assert!(text.contains("$10.00 · $20.00 (5% off)"), "{text}");
        assert!(text.contains("#3   Pro · $20 / month · $10.00 of usage, resets monthly · pay by card or balance"), "{text}");
    }
}
