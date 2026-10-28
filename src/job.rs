//! The job state machine. One transition function; it owns the concurrency limit.
//! Events come from the command line (Begin) and the network (Reply); effects are Send and Finish.
use serde_json::Value;
use std::collections::BTreeMap;
use std::path::PathBuf;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Model {
    ClefFlash,
    Clef,
    Clairic,
}

impl Model {
    pub fn name(self) -> &'static str {
        match self {
            Model::ClefFlash => "clef-flash",
            Model::Clef => "clef",
            Model::Clairic => "clairic",
        }
    }
    /// Clef takes `questions`; Clairic takes `fields`.
    pub fn schema_key(self) -> &'static str {
        match self {
            Model::Clairic => "fields",
            _ => "questions",
        }
    }
}

#[derive(Clone, Debug)]
pub struct Req {
    pub id: usize,
    pub group: String, // what is looked at: the shot's file name
    pub label: String, // who looks: the persona's file stem
    pub model: Model,
    pub state: String,
    pub schema: Value,
    pub images: Vec<PathBuf>,
}

pub type Outcome = Result<Value, String>;

#[derive(Debug)]
pub enum State {
    Idle,
    Running { queued: Vec<Req>, inflight: BTreeMap<usize, Req>, done: BTreeMap<usize, (Req, Outcome)> },
    Finished,
}

pub enum Event {
    Begin { reqs: Vec<Req>, limit: usize },
    Reply { id: usize, outcome: Outcome },
}

#[derive(Debug)]
pub enum Effect {
    Send(Req),
    Finish(Vec<(Req, Outcome)>),
}

const DEFAULT_LIMIT: usize = 4;

pub fn transition(state: State, event: Event) -> (State, Vec<Effect>) {
    match (state, event) {
        (State::Idle, Event::Begin { mut reqs, limit }) => {
            if reqs.is_empty() {
                return (State::Finished, vec![Effect::Finish(vec![])]);
            }
            let take = limit.max(1).min(reqs.len());
            let rest = reqs.split_off(take);
            // `limit` is remembered by how many are in flight: replies refill one for one.
            let effects = reqs.iter().cloned().map(Effect::Send).collect();
            let inflight = reqs.into_iter().map(|r| (r.id, r)).collect();
            let mut queued = rest;
            queued.reverse(); // pop from the back = original order
            (State::Running { queued, inflight, done: BTreeMap::new() }, effects)
        }
        (State::Running { mut queued, mut inflight, mut done }, Event::Reply { id, outcome }) => {
            let Some(req) = inflight.remove(&id) else {
                return (State::Running { queued, inflight, done }, vec![]); // a reply we did not ask for
            };
            done.insert(id, (req, outcome));
            let mut effects = vec![];
            if let Some(next) = queued.pop() {
                inflight.insert(next.id, next.clone());
                effects.push(Effect::Send(next));
            }
            if queued.is_empty() && inflight.is_empty() {
                let all = done.into_values().collect();
                return (State::Finished, vec![Effect::Finish(all)]);
            }
            (State::Running { queued, inflight, done }, effects)
        }
        (s, _) => (s, vec![]), // Begin twice, or a reply after the end: ignored
    }
}

pub fn default_limit() -> usize {
    DEFAULT_LIMIT
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn req(id: usize) -> Req {
        Req { id, group: "a.png".into(), label: format!("p{id}"), model: Model::ClefFlash, state: String::new(), schema: json!({}), images: vec![] }
    }

    #[test]
    fn sends_at_most_the_limit_then_refills_one_for_one() {
        let (s, fx) = transition(State::Idle, Event::Begin { reqs: (0..5).map(req).collect(), limit: 2 });
        assert_eq!(fx.len(), 2);
        let (s, fx) = transition(s, Event::Reply { id: 0, outcome: Ok(json!(1)) });
        assert!(matches!(fx.as_slice(), [Effect::Send(r)] if r.id == 2));
        let (s, _) = transition(s, Event::Reply { id: 1, outcome: Err("x".into()) });
        let (s, _) = transition(s, Event::Reply { id: 2, outcome: Ok(json!(1)) });
        let (s, _) = transition(s, Event::Reply { id: 3, outcome: Ok(json!(1)) });
        let (s, fx) = transition(s, Event::Reply { id: 4, outcome: Ok(json!(1)) });
        assert!(matches!(s, State::Finished));
        match fx.as_slice() {
            [Effect::Finish(all)] => assert_eq!(all.iter().map(|(r, _)| r.id).collect::<Vec<_>>(), vec![0, 1, 2, 3, 4]),
            _ => panic!("expected Finish"),
        }
    }

    #[test]
    fn empty_job_finishes_and_stray_replies_are_ignored() {
        let (s, fx) = transition(State::Idle, Event::Begin { reqs: vec![], limit: 4 });
        assert!(matches!(fx.as_slice(), [Effect::Finish(v)] if v.is_empty()));
        let (_, fx) = transition(s, Event::Reply { id: 9, outcome: Ok(json!(1)) });
        assert!(fx.is_empty());
    }
}
