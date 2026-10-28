//! gutcheck: cheap design gut-checks from Workers AI. The surface only turns arguments into events,
//! runs the effects the state machine asks for, and prints. Decisions live in job.rs.
mod api;
mod budget;
mod job;
mod render;
mod schema;

use clap::{Parser, Subcommand};
use job::{default_limit, transition, Effect, Event, Model, Req, State};
use serde_json::Value;
use std::{fs, path::{Path, PathBuf}, sync::mpsc, thread};

#[derive(Parser)]
#[command(name = "gutcheck", about = "Show Clef-flash or Clairic a screenshot with a persona and read back probabilities. They tell you where to look; they never approve a design.")]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Clef-flash: for-me, trust, want-to-try, polished, confused and the first feeling, per persona.
    Check {
        #[arg(required = true)]
        shots: Vec<PathBuf>,
        /// clef-flash (fast, default) or clef (the big model: use it for the home page, onboarding and the sales page)
        #[arg(long, default_value = "clef-flash")]
        model: String,
        #[command(flatten)]
        who: Who,
    },
    /// Score design directions while they are still words: each `## name` in a markdown file, every persona, in one batch.
    /// score = try + remember + trust - confused, mean over the buyers (try-*).
    Directions {
        /// Markdown file: one `## name` heading per direction; the text under it says what the visitor sees and does.
        file: PathBuf,
        /// clef (the big model, default) or clef-flash
        #[arg(long, default_value = "clef")]
        model: String,
        #[command(flatten)]
        who: Who,
    },
    /// Clairic: a 0-10 polish estimate (median, p10-p90) per named part of a screenshot, per persona.
    Polish {
        shot: PathBuf,
        /// JSON file: {"part_id": "what it is on screen", ...}
        parts: PathBuf,
        #[command(flatten)]
        who: Who,
    },
    /// Spend this month against the cap.
    Status,
    /// Send your own schema to a model and print the raw answers.
    Ask {
        /// clef-flash, clef or clairic
        #[arg(long, default_value = "clef-flash")]
        model: String,
        /// JSON file: the questions (Clef) or fields (Clairic)
        #[arg(long)]
        schema: PathBuf,
        /// The state text, or @file to read it from a file
        #[arg(long, default_value = "")]
        state: String,
        #[arg(long = "image")]
        images: Vec<PathBuf>,
    },
}

#[derive(clap::Args)]
struct Who {
    /// A persona file; repeat for several. Default: docs/personas/try-*.md (and wont-*.md for `check`), found from here upward.
    #[arg(long = "persona")]
    personas: Vec<PathBuf>,
    /// Where the persona files live, instead of docs/personas found from here upward.
    #[arg(long)]
    personas_dir: Option<PathBuf>,
    /// Print JSON for other programs instead of the table.
    #[arg(long)]
    json: bool,
    /// Where this person is, in one sentence, instead of the default website framing (e.g. "This person has just signed in to their bookkeeping app on their phone and sees this screen:").
    #[arg(long)]
    frame: Option<String>,
    /// Calls at once.
    #[arg(long, default_value_t = default_limit())]
    parallel: usize,
    /// `check` only: ask each persona about each shot N times and print the mean and spread, since one answer is noisy.
    #[arg(long, default_value_t = 1)]
    repeat: usize,
}

fn fail(msg: impl std::fmt::Display) -> ! {
    eprintln!("gutcheck: {msg}");
    std::process::exit(2)
}

fn find_personas_dir(from: &Path) -> Option<PathBuf> {
    from.ancestors().map(|a| a.join("docs/personas")).find(|p| p.is_dir())
}

fn personas(who: &Who, with_wont: bool) -> Vec<PathBuf> {
    if !who.personas.is_empty() {
        return who.personas.clone();
    }
    let dir = who.personas_dir.clone().or_else(|| find_personas_dir(&std::env::current_dir().unwrap_or_default()))
        .unwrap_or_else(|| fail("no docs/personas folder found here or above. Pass --persona FILE or --personas-dir DIR."));
    let mut names: Vec<PathBuf> = fs::read_dir(&dir).unwrap_or_else(|e| fail(format!("{}: {e}", dir.display())))
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|x| x == "md"))
        .filter(|p| {
            let n = p.file_name().unwrap().to_string_lossy();
            !n.contains("interview") && (n.starts_with("try-") || (with_wont && n.starts_with("wont-")))
        })
        .collect();
    // try-* before wont-*, each sorted, as the Python did
    names.sort_by_key(|p| (p.file_name().unwrap().to_string_lossy().starts_with("wont-"), p.clone()));
    if names.is_empty() {
        fail(format!("no persona files (try-*.md) in {}", dir.display()));
    }
    names
}

/// Exit 1 when any call failed, so a script can tell; the table already says which.
fn exit_for(all: &[(Req, job::Outcome)]) {
    if all.iter().any(|(_, o)| o.is_err()) {
        std::process::exit(1);
    }
}

fn clef_model(name: &str) -> Model {
    match name {
        "clef-flash" => Model::ClefFlash,
        "clef" => Model::Clef,
        other => fail(format!("unknown model {other:?} here; use clef or clef-flash")),
    }
}

fn stem(p: &Path) -> String {
    p.file_stem().unwrap_or_default().to_string_lossy().into_owned()
}

fn read(p: &Path) -> String {
    fs::read_to_string(p).unwrap_or_else(|e| fail(format!("could not read {}: {e}", p.display())))
}

/// Run the state machine to its end, performing each Send on a thread.
fn run(reqs: Vec<Req>, limit: usize) -> Vec<(Req, job::Outcome)> {
    let token = api::token().unwrap_or_else(|e| fail(e));
    let account = api::account().unwrap_or_else(|e| fail(e));
    let cap = budget::cap().unwrap_or_else(|e| fail(e));
    let batch: f64 = reqs.iter().map(budget::estimate_usd).sum();
    if let budget::Gate::Refuse { spent, batch, cap } = budget::gate(budget::spent_this_month(), batch, cap) {
        eprintln!("gutcheck: refused at the monthly cap: ${spent:.4} spent this month plus ${batch:.4} for this run is over ${cap:.2}. Raise it with GUTCHECK_CAP_USD=<dollars> or per_month_usd under [gutcheck] in the file GUTCHECK_BUDGET names.");
        std::process::exit(3);
    }
    let (tx, rx) = mpsc::channel::<(usize, job::Outcome)>();
    let mut state = State::Idle;
    let mut pending = vec![Event::Begin { reqs, limit }];
    loop {
        let event = match pending.pop() {
            Some(e) => e,
            None => {
                let (id, outcome) = rx.recv().expect("workers dropped");
                Event::Reply { id, outcome }
            }
        };
        let (next, effects) = transition(state, event);
        state = next;
        for fx in effects {
            match fx {
                Effect::Send(req) => {
                    let (tx, token, account) = (tx.clone(), token.clone(), account.clone());
                    thread::spawn(move || {
                        let out = api::send(&req, &account, &token);
                        budget::record(&req, out.is_ok());
                        let _ = tx.send((req.id, out));
                    });
                }
                Effect::Finish(all) => return all,
            }
        }
    }
}

fn main() {
    let cli = Cli::parse();
    match cli.cmd {
        Cmd::Check { shots, model, who } => {
            let model = clef_model(&model);
            let people = personas(&who, true);
            let mut reqs = vec![];
            let repeat = who.repeat.max(1);
            for s in &shots {
                for p in &people {
                    for _ in 0..repeat {
                        reqs.push(Req { id: reqs.len(), group: s.file_name().unwrap().to_string_lossy().into(), label: stem(p), model,
                            state: schema::gutcheck_state(&read(p), who.frame.as_deref()), schema: schema::gutcheck_questions(), images: vec![s.clone()] });
                    }
                }
            }
            let all = run(reqs, who.parallel);
            print!("{}", if who.json { render::json_out(&all) + "\n" } else if repeat > 1 { render::gutcheck_repeat_table(&all, repeat) } else { render::gutcheck_table(&all) });
            exit_for(&all);
        }
        Cmd::Directions { file, model, who } => {
            let model = clef_model(&model);
            let directions = schema::parse_directions(&read(&file));
            if directions.is_empty() {
                fail(format!("no `## name` headings in {}", file.display()));
            }
            let people = personas(&who, true);
            let mut reqs = vec![];
            for (name, text) in &directions {
                for p in &people {
                    reqs.push(Req { id: reqs.len(), group: name.clone(), label: stem(p), model,
                        state: schema::directions_state(&read(p), text, who.frame.as_deref()), schema: schema::directions_questions(), images: vec![] });
                }
            }
            let all = run(reqs, who.parallel);
            print!("{}", if who.json { render::json_out(&all) + "\n" } else { render::directions_table(&all, model.name()) });
            exit_for(&all);
        }
        Cmd::Polish { shot, parts, who } => {
            let text = read(&parts);
            let v: Value = serde_json::from_str(&text).unwrap_or_else(|e| fail(format!("{} is not JSON: {e}", parts.display())));
            let map = v.as_object().unwrap_or_else(|| fail(format!("{} must be an object of part_id: description", parts.display())));
            let names: Vec<String> = map.keys().cloned().collect();
            let fields = schema::polish_fields(map);
            let reqs = personas(&who, false).into_iter().enumerate().map(|(i, p)| Req {
                id: i, group: shot.file_name().unwrap().to_string_lossy().into(), label: stem(&p), model: Model::Clairic,
                state: schema::polish_state(&read(&p)), schema: fields.clone(), images: vec![shot.clone()] }).collect();
            let all = run(reqs, who.parallel);
            print!("{}", if who.json { render::json_out(&all) + "\n" } else { render::polish_table(&all, &names) });
            exit_for(&all);
        }
        Cmd::Status => {
            let cap = budget::cap().unwrap_or_else(|e| fail(e));
            println!("gutcheck spend {}: ${:.4} of ${cap:.2} (estimated from text size and image count at Cloudflare's input prices)", budget::month(), budget::spent_this_month());
        }
        Cmd::Ask { model, schema, state, images } => {
            let model = match model.as_str() {
                "clef-flash" => Model::ClefFlash,
                "clef" => Model::Clef,
                "clairic" => Model::Clairic,
                other => fail(format!("unknown model {other:?}; use clef-flash, clef or clairic")),
            };
            let state = match state.strip_prefix('@') { Some(f) => read(Path::new(f)), None => state };
            let sch: Value = serde_json::from_str(&read(&schema)).unwrap_or_else(|e| fail(format!("{} is not JSON: {e}", schema.display())));
            let req = Req { id: 0, group: "ask".into(), label: "ask".into(), model, state, schema: sch, images };
            let all = run(vec![req], 1);
            match &all[0].1 {
                Ok(a) => println!("{}", serde_json::to_string_pretty(a).unwrap()),
                Err(e) => fail(e),
            }
        }
    }
}
