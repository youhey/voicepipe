# AGENTS.md

This document describes the development conventions and constraints for agents working on this repository.

## Project Overview

`voicepipe` is a small Rust CLI application designed to turn radio-style script JSON into narrated audio programs.

Full name:

```txt
Personal Voice Rendering Pipeline for Radio Scripts.
```

Short description:

```txt
A tiny pipeline that turns radio-style scripts into narrated audio programs.
```

`voicepipe` is the downstream renderer of the `digestpipe` → `radiopipe` pipeline.

```txt
digestpipe  -> structured digest JSON
radiopipe   -> personalized radio-style script JSON
voicepipe   -> narrated audio program
```

The first development target is local radio narration rendering: read a scenario JSON file produced by `radiopipe`, synthesize each section with local TTS, combine the generated audio with `ffmpeg`, and output an MP3 file.

Do not add GUI features, podcast publishing, public web APIs, cloud job workers, mobile apps, or distribution automation unless explicitly requested.

## Initial Technical Scope

Use these assumptions unless the user explicitly changes them:

- Application type: CLI application.
- Language: Rust.
- Target language for narration: Japanese only.
- Input: JSON file downloaded/exported from `radiopipe`.
- TTS engine: local VOICEVOX Engine.
- Audio processing: `ffmpeg` command-line tool.
- Intermediate output: one WAV file per segment/section.
- Final output: MP3.
- Primary target machine: MacBook Air M2, 2022, 16GB memory class.
- GUI: none.

## Responsibility Boundaries

`voicepipe` should handle audio rendering concerns only.

Scenario JSON is responsible for:

- Episode identity.
- Episode title.
- Language.
- Section order.
- Section type.
- Section title.
- Section text.
- Estimated duration, when available.

`voicepipe` configuration is responsible for:

- VOICEVOX endpoint.
- Default speaker.
- Speed scale.
- Pitch scale.
- Intonation scale.
- Volume scale.
- Default section pause.
- Output bitrate.
- Work directory.
- Output directory.

Do not require voice settings to be embedded in the scenario JSON unless there is a clear content-level reason, such as scripted silence, jingle cues, or speaker changes that are part of the program itself.

## Repository Layout

Keep the repository root small and focused.

Suggested structure:

```txt
src/                 # Rust source code
crates/              # Optional internal crates, only if the project grows
config/              # Example configuration files
docs/                # Development documents
samples/             # Safe sample input JSON; no private data
scripts/             # Local helper scripts
README.md
AGENTS.md
Cargo.toml
Cargo.lock
```

Do not introduce a Laravel, PHP, Node, Electron, or web application structure unless explicitly requested.

## Rust Project Rules

Use stable Rust unless there is a specific reason to require nightly.

Commit `Cargo.lock` because this repository produces an application binary, not a library crate.

Prefer a straightforward module structure before introducing multiple crates.

Suggested initial modules:

```txt
src/main.rs          # CLI entry point
src/cli.rs           # clap argument definitions
src/config.rs        # Configuration loading and defaults
src/scenario.rs      # Input JSON models
src/validator.rs     # Scenario validation
src/voicevox.rs      # VOICEVOX HTTP client
src/renderer.rs      # Rendering orchestration
src/audio.rs         # Audio file planning and paths
src/ffmpeg.rs        # ffmpeg command wrapper
src/cache.rs         # Optional text/audio cache
src/error.rs         # Error types
```

Avoid over-engineering early. Do not add plugin systems, databases, async job queues, or background daemons unless needed by a concrete requirement.

## Rust Code Style

Use `rustfmt` for formatting.

Use `clippy` for linting.

Prefer clear, explicit data structures over clever abstractions.

Use `Result` and typed errors for recoverable failures. Do not `panic!` for normal input, configuration, TTS, or ffmpeg errors.

Use `anyhow` for application-level error propagation if the codebase is still small. Introduce `thiserror` for typed domain errors when contracts become stable.

Use `serde` / `serde_json` for JSON input.

Use `clap` for CLI parsing.

Use `reqwest` for VOICEVOX HTTP calls unless a lighter HTTP client is intentionally chosen.

Do not log full scenario JSON by default. Logs may include episode key, section id/title, section index, status, duration, and file paths.

Comments should explain intent, constraints, or non-obvious audio/TTS behavior. Do not add comments that merely restate the code.

User-facing CLI messages may be Japanese, because the target narration workflow is Japanese-only.

## CLI Design

Prefer a small command surface.

Initial command shape:

```bash
voicepipe render \
  --input ./episode.json \
  --output ./dist/episode.mp3 \
  --config ./config/voicepipe.toml
```

Useful options:

```txt
--input                 Input scenario JSON file
--output                Output MP3 path
--config                Configuration file path
--workdir               Working directory override
--voicevox-endpoint     VOICEVOX Engine endpoint override
--speaker               VOICEVOX speaker id override
--force                 Regenerate cached/intermediate audio
--dry-run               Validate and print render plan without synthesis
--keep-workdir          Keep intermediate files after successful render
```

Do not require interactive prompts for normal operation.

## Configuration

Prefer TOML for local configuration.

Suggested example file:

```toml
[voicevox]
endpoint = "http://127.0.0.1:50021"
default_speaker = 3

[voice]
speed_scale = 1.0
pitch_scale = 0.0
intonation_scale = 1.0
volume_scale = 1.0

[audio]
pause_between_sections_ms = 800
bitrate = "192k"
format = "mp3"

[output]
workdir = "./work"
distdir = "./dist"
```

Do not commit developer-specific local config files. Commit example config only.

## Input JSON Handling

The initial supported input is the `radiopipe` episode export shape.

Read only the fields required for rendering:

```txt
schema_version
episode.episode_key
episode.title
episode.language
episode.scenario_json.title
episode.scenario_json.language
episode.scenario_json.sections[]
episode.scenario_json.sections[].type
episode.scenario_json.sections[].title
episode.scenario_json.sections[].text
episode.scenario_json.sections[].estimated_duration_seconds
```

Validation requirements:

- `episode.episode_key` must be present and safe for path use after sanitization.
- Language must be `ja`.
- `scenario_json.sections` must contain at least one section.
- Each section must have non-empty `text`.
- Section text should be normalized before hashing/synthesis.
- Unknown extra fields must be tolerated.

Do not make `voicepipe` depend on upstream topic selection, editorial status, screening status, or article metadata unless explicitly requested.

## radiopipe Web API Contract

When implementing or changing code that consumes the public radiopipe Web API, treat the OpenAPI document in the radiopipe repository as the source of truth:

```txt
https://github.com/youhey/radiopipe/blob/main/docs/openapi.yaml
```

Public Web API request/response shapes should not be changed from the `voicepipe` side unless the task explicitly includes a radiopipe API feature change. If the API contract appears inconsistent with local assumptions, confirm against the OpenAPI document before changing parser structs, validation, or documentation.

## JSON Schema

Maintain a small JSON Schema for the subset of the `radiopipe` export consumed by `voicepipe`.

The schema should validate the rendering contract, not the entire upstream data model.

Use Rust-side validation for checks that are easier or clearer in code, such as path safety, text length limits, and render plan consistency.

## VOICEVOX Integration

Use VOICEVOX Engine as a local HTTP service.

The standard synthesis flow is:

```txt
POST /audio_query?text=...&speaker=...
POST /synthesis?speaker=...
```

Apply configured voice parameters by modifying the `audio_query` response before calling `/synthesis`.

Check VOICEVOX Engine availability before starting a full render.

Fail clearly if the engine is not reachable, the speaker id is invalid, or synthesis fails.

Do not bundle VOICEVOX Engine binaries, models, or speaker data into this repository unless explicitly requested.

VOICEVOX output and character/voice usage may have licensing requirements. Do not claim generated audio is freely usable without confirming the relevant voice library terms.

VOICEVOX Engine is not part of `voicepipe`. Treat it as an external TTS backend.

For local development, keep Docker operations for VOICEVOX Engine in `Makefile` targets such as `make voicevox-up`, `make voicevox-down`, `make voicevox-logs`, and `make voicevox-status`.

## Audio Rendering

Render section audio as separate WAV files first.

Then use `ffmpeg` to:

- Insert configured silence between sections.
- Concatenate section WAV files.
- Encode the final MP3.

Prefer deterministic file names based on section order, section type, and a short content hash.

Suggested workdir layout:

```txt
work/<episode_key>/
  segments/
    000-opening-<hash>.wav
    001-topic-<hash>.wav
  silence/
    pause-0800ms.wav
  concat.ffconcat
  combined.wav
  render-plan.json
```

Do not commit generated audio or work directories.

## Caching

Cache generated section WAV files by a stable key that includes at least:

- Normalized section text.
- VOICEVOX speaker id.
- Voice parameters.
- VOICEVOX Engine version if available.
- `voicepipe` cache schema version.

A text-only cache key is not sufficient if speaker or voice parameters can change.

Provide `--force` to ignore/rebuild cached audio.

## ffmpeg

Treat `ffmpeg` as an external runtime dependency.

Check that `ffmpeg` is available before rendering.

Do not assume Homebrew paths. Prefer invoking `ffmpeg` from `PATH`, with a config/CLI override only if needed later.

Capture stderr on failure and summarize it. Do not dump excessively long ffmpeg output unless verbose mode is enabled.

## Testing

Use Rust tests for parsing, validation, render planning, cache keys, and path generation.

Automated tests must not call real VOICEVOX Engine by default.

Use fixtures and mocked HTTP clients for VOICEVOX behavior.

Do not require `ffmpeg` for ordinary unit tests. Keep ffmpeg-dependent tests ignored or behind an explicit feature/flag.

Suggested commands:

```bash
cargo test
cargo fmt --check
cargo clippy --all-targets --all-features -- -D warnings
cargo audit
```

If a `Makefile` or `justfile` is added, keep it as a thin wrapper around these commands.

## Sample Data

Safe sample files may be committed under `samples/`.

Do not commit private/generated real episode exports unless they are intentionally sanitized.

Do not commit files containing real secrets, private API URLs, unreleased source data, or large generated audio.

## Logging and Privacy

Logs should be useful for local debugging but not leak unnecessary content.

Allowed in normal logs:

- Episode key.
- Section index.
- Section type.
- Section title.
- Render status.
- Output file path.
- Duration and elapsed time.

Avoid in normal logs:

- Full scenario JSON.
- Full section text.
- Secrets or tokens.
- Full external command output unless verbose mode is enabled.

## Dependencies

Keep dependencies minimal.

Likely initial dependencies:

```txt
clap
serde
serde_json
serde_with
toml
reqwest
tokio
anyhow
thiserror
tracing
tracing-subscriber
sha2
sanitize-filename
```

Before adding heavy dependencies, prefer standard library solutions or small crates.

Do not add audio decoding/encoding libraries until there is a clear need; use `ffmpeg` for audio processing in the initial implementation.

## Platform Notes

Primary development target is Apple Silicon macOS.

Do not add Linux-only assumptions unless guarded or documented.

Avoid hard-coding absolute local paths.

Keep file paths UTF-8 friendly, but sanitize output names derived from episode keys or titles.

## Documentation

When behavior changes, update `README.md` in the same task.

When input schema changes, update the JSON Schema and relevant docs in the same task.

Document required local tools:

- Rust toolchain.
- VOICEVOX Engine.
- ffmpeg.

Document the expected render flow:

```txt
radiopipe JSON -> VOICEVOX section WAVs -> ffmpeg concat -> MP3
```

Do not document planned features as current behavior.

## Out of Scope Unless Requested

The following are intentionally out of scope for initial development:

- GUI.
- Web server.
- Laravel/PHP integration.
- Database storage.
- Cloud queues.
- Podcast RSS publishing.
- S3 upload.
- BGM/SE mixing.
- Multi-language narration.
- Multiple TTS providers.
- Real-time streaming playback.
- Automatic script generation.
- Upstream news fetching.
- Editorial selection logic.

## Agent Behavior

Before making broad architectural changes, check whether they fit the current narrow scope.

Prefer small, reviewable changes.

Do not rewrite existing README/project wording casually. Preserve the chosen naming:

```txt
voicepipe
Personal Voice Rendering Pipeline for Radio Scripts.
A tiny pipeline that turns radio-style scripts into narrated audio programs.
```

Keep `Radio Narration Rendering` as a useful concept phrase in documentation where it helps explain the project.
