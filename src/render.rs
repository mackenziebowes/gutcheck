//! Outcomes to text. Pure.
use crate::job::{Outcome, Req};
use serde_json::{json, Value};

fn num(v: &Value, k: &str) -> Option<f64> {
    v.get(k)?.get("noul")?.as_f64()
}

fn f2(x: Option<f64>) -> String {
    x.map(|n| format!("{n:.2}")).unwrap_or_else(|| "?".into())
}

pub fn gutcheck_table(all: &[(Req, Outcome)]) -> String {
    let mut out = String::new();
    let mut group = None::<&str>;
    for (r, o) in all {
        if group != Some(r.group.as_str()) {
            group = Some(&r.group);
            out += &format!("== {}\n   {:28} {:>7} {:>6} {:>6} {:>7} {:>9}  feels first\n", r.group, "persona", "for me", "trust", "try", "polish", "confused");
        }
        match o {
            Err(e) => out += &format!("   {:28} {e}\n", r.label),
            Ok(a) => {
                let ff = a.get("feel_first").cloned().unwrap_or(json!({}));
                let choice = ff.get("choice").and_then(Value::as_str).unwrap_or("?");
                let conf = ff.get("confidence").and_then(Value::as_f64).unwrap_or(0.0);
                out += &format!("   {:28} {:>7} {:>6} {:>6} {:>7} {:>9}  {choice} ({conf:.2})\n",
                    r.label, f2(num(a, "for_me")), f2(num(a, "trust")), f2(num(a, "want_to_try")), f2(num(a, "polished")), f2(num(a, "confused")));
            }
        }
    }
    out
}

fn mean_sd(v: &[f64]) -> Option<(f64, f64)> {
    if v.is_empty() {
        return None;
    }
    let n = v.len() as f64;
    let m = v.iter().sum::<f64>() / n;
    Some((m, (v.iter().map(|x| (x - m).powi(2)).sum::<f64>() / n).sqrt()))
}

fn ms(v: &[f64]) -> String {
    mean_sd(v).map(|(m, s)| format!("{m:.2}±{s:.2}")).unwrap_or_else(|| "?".into())
}

/// With `--repeat N`: per shot and persona, mean±spread of each score over the N answers, then the mean over personas.
pub fn gutcheck_repeat_table(all: &[(Req, Outcome)], n: usize) -> String {
    let mut out = String::new();
    let mut groups: Vec<&str> = vec![];
    for (r, _) in all {
        if !groups.contains(&r.group.as_str()) {
            groups.push(&r.group);
        }
    }
    let keys = ["for_me", "trust", "want_to_try", "polished", "confused"];
    for g in groups {
        out += &format!("== {g} (mean±spread over {n} answers)\n   {:28} {:>11} {:>11} {:>11} {:>11} {:>11}\n", "persona", "for me", "trust", "try", "polish", "confused");
        let rows: Vec<_> = all.iter().filter(|(r, _)| r.group == g).collect();
        let mut labels: Vec<&str> = vec![];
        for (r, _) in &rows {
            if !labels.contains(&r.label.as_str()) {
                labels.push(&r.label);
            }
        }
        let mut means: Vec<Vec<f64>> = vec![vec![]; keys.len()];
        for l in labels {
            let mine: Vec<_> = rows.iter().filter(|(r, _)| r.label == l).collect();
            let errs = mine.iter().filter(|(_, o)| o.is_err()).count();
            let col: Vec<Vec<f64>> = keys.iter().map(|k| mine.iter().filter_map(|(_, o)| num(o.as_ref().ok()?, k)).collect()).collect();
            for (i, c) in col.iter().enumerate() {
                if let Some((m, _)) = mean_sd(c) {
                    means[i].push(m);
                }
            }
            out += &format!("   {l:28} {:>11} {:>11} {:>11} {:>11} {:>11}{}\n", ms(&col[0]), ms(&col[1]), ms(&col[2]), ms(&col[3]), ms(&col[4]),
                if errs > 0 { format!("  ({errs} failed)") } else { String::new() });
        }
        let mm = |i: usize| mean_sd(&means[i]).map(|(m, _)| format!("{m:.2}")).unwrap_or_else(|| "?".into());
        out += &format!("   {:28} {:>11} {:>11} {:>11} {:>11} {:>11}\n", "mean over personas", mm(0), mm(1), mm(2), mm(3), mm(4));
    }
    out
}

fn cell(v: &Value) -> String {
    let med = v.get("median").or_else(|| v.get("value"));
    let med = med.map(|m| m.to_string()).unwrap_or_else(|| "?".into());
    match v.get("p10") {
        Some(lo) => format!("{med} ({lo}-{})", v.get("p90").map(|x| x.to_string()).unwrap_or_default()),
        None => med,
    }
}

/// `parts` is in the order the file gave them.
pub fn polish_table(all: &[(Req, Outcome)], parts: &[String]) -> String {
    let mut out = String::new();
    let ok: Vec<_> = all.iter().filter_map(|(r, o)| o.as_ref().ok().map(|a| (r, a))).collect();
    for (r, o) in all {
        if let Err(e) = o {
            out += &format!("{} {e}\n", r.label);
        }
    }
    if ok.is_empty() {
        return out;
    }
    out += &format!("{:22}", "part");
    for (r, _) in &ok {
        out += &format!("{:>16}", short(&r.label));
    }
    out.push('\n');
    for p in parts {
        out += &format!("{p:22}");
        for (_, a) in &ok {
            out += &format!("{:>16}", cell(a.get(p).unwrap_or(&Value::Null)));
        }
        out.push('\n');
    }
    out
}

/// `try-1-kahlil-dennis` -> `kahlil` as the old polish.py did; any other name stays whole.
fn short(stem: &str) -> String {
    stem.split('-').nth(2).unwrap_or(stem).to_string()
}

/// score = try + remember + trust - confused, mean over the buyers (`try-*`); the number to climb.
/// Non-buyers (`wont-*`) are shown only as their mean "for me".
pub fn directions_table(all: &[(Req, Outcome)], model: &str) -> String {
    let mut names: Vec<&str> = vec![];
    for (r, _) in all {
        if !names.contains(&r.group.as_str()) {
            names.push(&r.group);
        }
    }
    let is_buyer = |r: &Req| !r.label.starts_with("wont-");
    let buyers = all.iter().filter(|(r, _)| is_buyer(r)).map(|(r, _)| &r.label).collect::<std::collections::BTreeSet<_>>().len();
    let others = all.iter().filter(|(r, _)| !is_buyer(r)).map(|(r, _)| &r.label).collect::<std::collections::BTreeSet<_>>().len();
    let mut out = format!("model {model}; mean over {buyers} buyers (try-*), and over {others} non-buyers (wont-*) for 'for me'\n");
    out += &format!("{:22} {:>6} {:>7} {:>6} {:>6} {:>9} {:>9}   non-buyers for me\n", "direction", "score", "for me", "trust", "try", "remember", "confused");
    for n in names {
        let rows: Vec<_> = all.iter().filter(|(r, _)| r.group == n).collect();
        if let Some(e) = rows.iter().find_map(|(_, o)| o.as_ref().err()) {
            out += &format!("{n:22} {e}\n");
            continue;
        }
        let mean = |k: &str, buyer: bool| -> Option<f64> {
            let v: Vec<f64> = rows.iter().filter(|(r, _)| is_buyer(r) == buyer).filter_map(|(_, o)| num(o.as_ref().ok()?, k)).collect();
            (!v.is_empty()).then(|| v.iter().sum::<f64>() / v.len() as f64)
        };
        let b = |k| mean(k, true).unwrap_or(f64::NAN);
        let score = b("try") + b("remember") + b("trust") - b("confused");
        let others = mean("for_me", false).map(|x| format!("{x:.2}")).unwrap_or_else(|| "nan".into());
        out += &format!("{n:22} {score:6.2} {:7.2} {:6.2} {:6.2} {:9.2} {:9.2}   {others}\n", b("for_me"), b("trust"), b("try"), b("remember"), b("confused"));
    }
    out
}

pub fn json_out(all: &[(Req, Outcome)]) -> String {
    let rows: Vec<Value> = all.iter().map(|(r, o)| match o {
        Ok(a) => json!({"group": r.group, "label": r.label, "model": r.model.name(), "answers": a}),
        Err(e) => json!({"group": r.group, "label": r.label, "model": r.model.name(), "error": e}),
    }).collect();
    serde_json::to_string_pretty(&rows).unwrap()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::job::Model;

    fn req(label: &str) -> Req {
        Req { id: 0, group: "s.png".into(), label: label.into(), model: Model::ClefFlash, state: String::new(), schema: json!({}), images: vec![] }
    }

    #[test]
    fn gutcheck_row_matches_the_python_layout() {
        let a = json!({"for_me": {"noul": 0.39}, "trust": {"noul": 0.42}, "want_to_try": {"noul": 0.21}, "polished": {"noul": 0.37}, "confused": {"noul": 0.44}, "feel_first": {"choice": "relief", "confidence": 0.09}});
        let t = gutcheck_table(&[(req("try-1-kahlil-dennis"), Ok(a))]);
        assert!(t.contains("   try-1-kahlil-dennis             0.39   0.42   0.21    0.37      0.44  relief (0.09)"), "{t}");
    }

    #[test]
    fn repeat_table_shows_mean_and_spread() {
        let a = |c: f64| json!({"for_me": {"noul": 0.5}, "trust": {"noul": 0.5}, "want_to_try": {"noul": 0.5}, "polished": {"noul": 0.5}, "confused": {"noul": c}});
        let t = gutcheck_repeat_table(&[(req("try-1-a"), Ok(a(0.4))), (req("try-1-a"), Ok(a(0.6)))], 2);
        assert!(t.contains("0.50±0.10"), "{t}");
    }

    #[test]
    fn polish_cell_shows_range_when_there_is_one() {
        assert_eq!(cell(&json!({"median": 7, "p10": 5, "p90": 9})), "7 (5-9)");
        assert_eq!(cell(&json!({"value": 6})), "6");
        assert_eq!(short("try-3-david-hayes"), "david");
    }
}
