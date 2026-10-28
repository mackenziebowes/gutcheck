//! The monthly spend cap. `gate` is pure; the rest reads and writes the call log.
//! Responses carry no token counts, so cost is an estimate: text at 3.5 characters a token, 2000 tokens an image,
//! priced from developers.cloudflare.com/workers-ai/models/<model> (input only; no output charge).
use crate::job::{Model, Req};
use std::{fs, io::Write, path::PathBuf};

#[derive(Debug, PartialEq)]
pub enum Gate {
    Allow,
    Refuse { spent: f64, batch: f64, cap: f64 },
}

pub fn gate(spent: f64, batch: f64, cap: f64) -> Gate {
    if spent + batch > cap { Gate::Refuse { spent, batch, cap } } else { Gate::Allow }
}

pub fn usd_per_million(m: Model) -> f64 {
    match m {
        Model::ClefFlash => 0.038,
        Model::Clef => 0.24,
        Model::Clairic => 0.10,
    }
}

pub fn estimate_usd(r: &Req) -> f64 {
    let tokens = r.state.len() as f64 / 3.5 + r.schema.to_string().len() as f64 / 3.5 + 2000.0 * r.images.len() as f64;
    tokens / 1e6 * usd_per_million(r.model)
}

fn home() -> PathBuf {
    std::env::var_os("HOME").map(Into::into).unwrap_or_default()
}

fn log_path() -> PathBuf {
    std::env::var_os("GUTCHECK_LOG").map(Into::into).unwrap_or_else(|| home().join(".local/share/gutcheck/calls.jsonl"))
}

fn budget_path() -> PathBuf {
    std::env::var_os("GUTCHECK_BUDGET").map(Into::into).unwrap_or_else(|| home().join(".config/gutcheck/budget.toml"))
}

pub const DEFAULT_CAP_USD: f64 = 10.0;

/// The monthly cap: GUTCHECK_CAP_USD, else per_month_usd in the [gutcheck] table of a budget file
/// (GUTCHECK_BUDGET, default ~/.config/gutcheck/budget.toml), else DEFAULT_CAP_USD.
pub fn cap() -> Result<f64, String> {
    if let Ok(v) = std::env::var("GUTCHECK_CAP_USD") {
        return parse_dollars(&v).map_err(|_| format!("GUTCHECK_CAP_USD is not a number of dollars (0 or more): {v}"));
    }
    let p = budget_path();
    let Ok(text) = fs::read_to_string(&p) else { return Ok(DEFAULT_CAP_USD) };
    Ok(parse_cap(&text).map_err(|v| format!("per_month_usd in {} is not a number: {v}", p.display()))?.unwrap_or(DEFAULT_CAP_USD))
}

/// A dollar amount as written in a budget file or the environment: an optional trailing `# comment`
/// and optional quotes are allowed. A cap must be finite and not negative, or the gate's comparison
/// (spent + batch > cap) is false for NaN and the cap silently turns off.
fn parse_dollars(raw: &str) -> Result<f64, ()> {
    let v = raw.split('#').next().unwrap_or("").trim().trim_matches(|c| c == '"' || c == '\'').trim();
    let n: f64 = v.parse().map_err(|_| ())?;
    if n.is_finite() && n >= 0.0 { Ok(n) } else { Err(()) }
}

fn parse_cap(text: &str) -> Result<Option<f64>, String> {
    let mut in_table = false;
    for line in text.lines().map(str::trim) {
        if line.starts_with('[') {
            in_table = line == "[gutcheck]";
        } else if in_table {
            if let Some(v) = line.strip_prefix("per_month_usd") {
                let v = v.trim_start().trim_start_matches('=').trim();
                return parse_dollars(v).map(Some).map_err(|_| v.to_string());
            }
        }
    }
    Ok(None)
}

pub fn month() -> String {
    let o = std::process::Command::new("date").arg("+%Y-%m").output().ok();
    o.map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string()).unwrap_or_default()
}

pub fn spent_this_month() -> f64 {
    let m = format!("\"month\":\"{}\"", month());
    fs::read_to_string(log_path()).unwrap_or_default().lines().filter(|l| l.contains(&m)).filter_map(|l| {
        let v: serde_json::Value = serde_json::from_str(l).ok()?;
        v.get("cost_usd")?.as_f64()
    }).sum::<f64>() + 0.0
}

pub fn record(r: &Req, ok: bool) {
    let p = log_path();
    let _ = fs::create_dir_all(p.parent().unwrap());
    let line = serde_json::json!({"month": month(), "model": r.model.name(), "group": r.group, "label": r.label, "cost_usd": estimate_usd(r), "estimated": true, "ok": ok});
    if let Ok(mut f) = fs::OpenOptions::new().create(true).append(true).open(p) {
        // One write call: O_APPEND keeps a single write whole, so two gutchecks cannot interleave bytes.
        let _ = f.write_all(format!("{line}\n").as_bytes());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cap_reads_the_gutcheck_table_and_otherwise_has_none() {
        assert_eq!(parse_cap("[aion]\nper_month_usd = 3\n[gutcheck]\nper_month_usd = 7.5\n"), Ok(Some(7.5)));
        assert_eq!(parse_cap("[aion]\nper_month_usd = 3\n"), Ok(None));
        assert!(parse_cap("[gutcheck]\nper_month_usd = lots\n").is_err());
    }

    #[test]
    fn cap_survives_a_comment_or_quotes_in_the_file() {
        assert_eq!(parse_cap("[gutcheck]\nper_month_usd = 25  # my cap\n"), Ok(Some(25.0)));
        assert_eq!(parse_cap("[gutcheck]\nper_month_usd = \"25\"\n"), Ok(Some(25.0)));
        assert_eq!(parse_cap("[gutcheck]\nper_month_usd = '7.5' # note\n"), Ok(Some(7.5)));
    }

    #[test]
    fn cap_rejects_values_that_would_turn_it_off() {
        for bad in ["nan", "NaN", "inf", "infinity", "-1", "-inf"] {
            assert!(parse_dollars(bad).is_err(), "{bad}");
            assert!(parse_cap(&format!("[gutcheck]\nper_month_usd = {bad}\n")).is_err(), "{bad}");
        }
        assert_eq!(parse_dollars(" 0 "), Ok(0.0));
    }

    #[test]
    fn refuses_only_when_the_batch_would_cross_the_cap() {
        assert_eq!(gate(9.0, 0.5, 10.0), Gate::Allow);
        assert_eq!(gate(10.0, 0.0, 10.0), Gate::Allow);
        assert!(matches!(gate(9.9, 0.2, 10.0), Gate::Refuse { .. }));
    }

    #[test]
    fn clef_costs_more_than_flash_for_the_same_request() {
        let mk = |model| Req { id: 0, group: String::new(), label: String::new(), model, state: "x".repeat(3500), schema: serde_json::json!({}), images: vec![] };
        assert!(estimate_usd(&mk(Model::Clef)) > estimate_usd(&mk(Model::ClefFlash)));
    }
}
