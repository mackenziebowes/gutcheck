# gutcheck

A cheap design gut-check. It shows Workers AI Clef-flash or Clairic a screenshot together with a buyer persona and prints
probabilities: would this person feel it is for them, trust it, want to try it, see it as polished, be confused, and what would
they feel first. The numbers tell you where to look. They never approve a design.

## Install

Needs [Rust](https://rustup.rs), a Cloudflare account (Workers AI has a free allowance) and, only for screenshots over 4 MiB, ImageMagick (`magick`).

    cargo install --git https://github.com/mackenziebowes/gutcheck
    export CLOUDFLARE_API_TOKEN=...        # a token with Workers AI permission, or run `wrangler login`
    export CLOUDFLARE_ACCOUNT_ID=...       # or put it on the first line of ~/.config/gutcheck/account_id
    gutcheck check shot.png --persona examples/personas/try-1-example.md

## Use

    gutcheck check shots/home-390.png [more.png] [--persona docs/personas/try-1-x.md]
    gutcheck check shot.png --model clef                 # the big model (default clef-flash): home page, onboarding, sales page
    gutcheck directions directions.md                    # plans still in words: each "## name" scored by every persona (big Clef)
    gutcheck polish shots/section.png parts.json        # parts.json: {"headline": "the big first sentence", ...}
    gutcheck ask --model clef --schema q.json --state @persona.md --image shot.png

- Personas: `docs/personas/try-*.md` (and `wont-*.md` for `check`), found from the current folder upward. `--personas-dir DIR` or `--persona FILE` instead.
- `--json` prints rows for other programs. `--parallel N` sets calls at once (4).
- Token: `CLOUDFLARE_API_TOKEN`, else the wrangler login in `~/.config/.wrangler/config/default.toml` (`wrangler login` if refused). Account: `CLOUDFLARE_ACCOUNT_ID`, else `~/.config/gutcheck/account_id`.
- A persona is a plain markdown file written in the second person ("You are ..."); see `examples/personas/`. The whole file is sent to the model, so keep it to the person.
- Cost: US$0.10 per million input tokens on Workers AI; a screenshot is a fraction of a cent per persona.
- The model reads questions in the order given, so the order is fixed (a test pins it). Clairic samples, so its ranges move a point between runs.

`directions` score = try + remember + trust - confused, mean over the buyers (every persona that is not `wont-*`); `wont-*` personas show only as their mean "for me".

Code: `job.rs` is the state machine (queue, concurrency, finish), `api.rs` the only code that touches the network, `render.rs` and `schema.rs` are pure.
MIT.

Spend cap: US$10 a month unless you set one. `GUTCHECK_CAP_USD=25` raises it; or put `per_month_usd = 25` under `[gutcheck]` in `~/.config/gutcheck/budget.toml` (or the file `GUTCHECK_BUDGET` names). Every call is logged to `~/.local/share/gutcheck/calls.jsonl` (`GUTCHECK_LOG` moves it); `gutcheck status` shows the month; a run that would cross the cap is refused with exit 3. Costs are estimates (the responses carry no token counts): text at 3.5 characters a token and 2000 tokens an image, at Cloudflare input prices (Clef $0.24, Clef-flash $0.038, Clairic $0.10 per million).
