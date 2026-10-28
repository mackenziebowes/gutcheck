//! The only code that touches the network, the token file or `magick`.
use crate::job::Req;
use base64::Engine;
use serde_json::{json, Value};
use std::{fs, path::Path, process::Command, time::Duration};

const MAX_IMAGE: usize = 4 * 1024 * 1024;

/// CLOUDFLARE_ACCOUNT_ID, else the first line of ~/.config/gutcheck/account_id.
pub fn account() -> Result<String, String> {
    if let Ok(a) = std::env::var("CLOUDFLARE_ACCOUNT_ID") {
        return Ok(a.trim().to_string());
    }
    let path = dirs_home().join(".config/gutcheck/account_id");
    fs::read_to_string(&path).ok().and_then(|t| t.lines().next().map(|l| l.trim().to_string())).filter(|a| !a.is_empty()).ok_or_else(|| {
        format!("no Cloudflare account id: set CLOUDFLARE_ACCOUNT_ID, or put it on the first line of {} (it is on the right of the Workers & Pages overview page).", path.display())
    })
}

pub fn token() -> Result<String, String> {
    if let Ok(t) = std::env::var("CLOUDFLARE_API_TOKEN") {
        return Ok(t);
    }
    let path = dirs_home().join(".config/.wrangler/config/default.toml");
    let text = fs::read_to_string(&path).map_err(|e| format!("no Cloudflare token: could not read {} ({e}). Run `wrangler login` or set CLOUDFLARE_API_TOKEN.", path.display()))?;
    text.lines()
        .find_map(|l| l.strip_prefix("oauth_token = \"").and_then(|r| r.strip_suffix('"')))
        .map(String::from)
        .ok_or_else(|| format!("no oauth_token line in {}. Run `wrangler login`.", path.display()))
}

fn dirs_home() -> std::path::PathBuf {
    std::env::var_os("HOME").map(Into::into).unwrap_or_default()
}

fn data_url(path: &Path) -> Result<String, String> {
    let mut data = fs::read(path).map_err(|e| format!("could not read image {}: {e}", path.display()))?;
    if data.len() > MAX_IMAGE {
        let tmp = std::env::temp_dir().join(format!("gutcheck-{}-{}.jpg", std::process::id(), data.len()));
        let ok = Command::new("magick")
            .args([path.as_os_str(), "-resize".as_ref(), "1600x1600>".as_ref(), "-quality".as_ref(), "85".as_ref(), tmp.as_os_str()])
            .status()
            .map(|s| s.success())
            .unwrap_or(false);
        if !ok {
            return Err(format!("{} is over 4 MiB and `magick` could not shrink it", path.display()));
        }
        data = fs::read(&tmp).map_err(|e| e.to_string())?;
        let _ = fs::remove_file(&tmp);
    }
    let kind = if data.starts_with(b"\x89PNG") { "png" } else { "jpeg" };
    Ok(format!("data:image/{kind};base64,{}", base64::engine::general_purpose::STANDARD.encode(data)))
}

pub fn body(req: &Req, images: Vec<String>) -> Value {
    let mut b = json!({ "model": req.model.name(), "state": req.state, "images": images });
    b[req.model.schema_key()] = req.schema.clone();
    if req.model == crate::job::Model::Clairic {
        b["reference_date"] = json!(chrono_free_today());
    }
    b
}

fn chrono_free_today() -> String {
    // Clairic wants a reference date; the day the call is made is the honest one.
    let out = Command::new("date").arg("+%F").output().ok();
    out.map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string()).unwrap_or_else(|| "2026-01-01".into())
}

/// One call, with two retries on 429 and 5xx. Returns the model's `answers`.
pub fn send(req: &Req, account: &str, token: &str) -> Result<Value, String> {
    let images = req.images.iter().map(|p| data_url(p)).collect::<Result<Vec<_>, _>>()?;
    let payload = body(req, images);
    let url = format!("https://api.cloudflare.com/client/v4/accounts/{account}/ai/run/@cf/cloudflare/{}", req.model.name());
    let mut last = String::new();
    for attempt in 0..3u64 {
        if attempt > 0 {
            std::thread::sleep(Duration::from_secs(2 * attempt));
        }
        let r = ureq::post(&url)
            .set("Authorization", &format!("Bearer {token}"))
            .set("User-Agent", "gutcheck")
            .timeout(Duration::from_secs(120))
            .send_json(&payload);
        match r {
            Ok(resp) => {
                let v: Value = resp.into_json().map_err(|e| format!("reply was not JSON: {e}"))?;
                let inner = v.get("result").unwrap_or(&v);
                return Ok(inner.get("answers").cloned().unwrap_or_else(|| inner.clone()));
            }
            Err(ureq::Error::Status(code, resp)) => {
                let text: String = resp.into_string().unwrap_or_default().chars().take(300).collect();
                last = format!("http {code}: {text}");
                if code == 401 || code == 403 {
                    return Err(format!("{last}. The Cloudflare token was refused; run `wrangler login` and retry."));
                }
                if code != 429 && code < 500 {
                    return Err(last);
                }
            }
            Err(e) => last = format!("network: {e}"),
        }
    }
    Err(last)
}
