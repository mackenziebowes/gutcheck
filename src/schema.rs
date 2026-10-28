//! What we ask the models. Pure data.
use serde_json::{json, Value};

pub fn gutcheck_questions() -> Value {
    json!({
        "for_me": {"type": "noul", "instructions": "Would this person feel this screen was made for someone like them?"},
        "trust": {"type": "noul", "instructions": "Would this person trust what this screen says?"},
        "want_to_try": {"type": "noul", "instructions": "Would this person want to tap or try something on this screen right now?"},
        "polished": {"type": "noul", "instructions": "Would this person see this as a polished, normal website from a real company?"},
        "confused": {"type": "noul", "instructions": "Would this person be confused about what to do here?"},
        "feel_first": {"type": "choice", "instructions": "What would this person feel first, looking at this screen?",
            "criteria": {
                "relief": "relief that someone gets it",
                "curiosity": "curiosity, wanting to see more",
                "doubt": "doubt that it will work for them",
                "boredom": "boredom, it looks like every other site",
                "confusion": "confusion about what it is",
                "shame": "shame about their own mess"}},
    })
}

pub fn gutcheck_state(persona: &str, frame: Option<&str>) -> String {
    let frame = frame.unwrap_or("This is a person looking at a screenshot of a website on their screen:");
    format!("{frame}\n\n{persona}")
}

pub fn polish_fields(parts: &serde_json::Map<String, Value>) -> Value {
    let mut out = serde_json::Map::new();
    for (k, v) in parts {
        let what = v.as_str().unwrap_or("");
        out.insert(k.clone(), json!({
            "type": "estimate", "min": 0, "max": 10, "step": 1,
            "instructions": format!("How polished and convincing this part looks to this person on their phone: {what}. 0 = unfinished or generic, 10 = feels like a top company made it for them.")
        }));
    }
    Value::Object(out)
}

pub fn polish_state(persona: &str) -> String {
    format!("{persona}\n\nThis person is on their phone, looking at this section of a website selling AI bookkeeping (screenshot attached).")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The models read questions in the order given: sorted keys moved for_me from 0.39 to 0.30 on the same shot.
    #[test]
    fn question_order_is_kept() {
        let keys: Vec<_> = gutcheck_questions().as_object().unwrap().keys().cloned().collect();
        assert_eq!(keys, ["for_me", "trust", "want_to_try", "polished", "confused", "feel_first"]);
    }
}

pub fn directions_questions() -> Value {
    json!({
        "for_me": {"type": "noul", "instructions": "Would this person feel this was made for someone like them?"},
        "trust": {"type": "noul", "instructions": "Would this person trust it?"},
        "try": {"type": "noul", "instructions": "Would this person take the photo or tap the button right now?"},
        "remember": {"type": "noul", "instructions": "Would this person remember this part of the page tomorrow?"},
        "confused": {"type": "noul", "instructions": "Would this person be confused about what to do?"},
    })
}

pub fn directions_state(persona: &str, direction: &str, frame: Option<&str>) -> String {
    let frame = frame.unwrap_or("This person is on a website for AI bookkeeping, on their phone, and scrolls to this part of the page:");
    format!("{persona}\n\n{frame}\n\n{direction}")
}

/// Each `## name` heading starts a direction; the text under it is what the visitor sees and does.
pub fn parse_directions(md: &str) -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> = vec![];
    for line in md.lines() {
        if let Some(name) = line.strip_prefix("## ") {
            out.push((name.trim().to_string(), String::new()));
        } else if let Some((_, body)) = out.last_mut() {
            body.push_str(line);
            body.push('\n');
        }
    }
    out.into_iter().map(|(n, b)| (n, b.trim().to_string())).collect()
}

#[cfg(test)]
mod direction_tests {
    use super::*;

    #[test]
    fn headings_split_directions_and_text_before_the_first_is_ignored() {
        let d = parse_directions("intro\n## one\nsees a photo\ntaps\n## two words\nsees a form\n");
        assert_eq!(d, vec![("one".into(), "sees a photo\ntaps".into()), ("two words".into(), "sees a form".into())]);
    }
}
