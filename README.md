# voicepipe

Personal Voice Rendering Pipeline for Radio Scripts.

A tiny pipeline that turns radio-style scripts into narrated audio programs.

voicepipe is the downstream renderer of the digestpipe → radiopipe pipeline.

It consumes structured radio scripts and produces narrated audio episodes using local text-to-speech engines such as VOICEVOX.

## Concept

Information
↓
digestpipe

Structured News Digest
↓
radiopipe

Radio Narration Script
↓
voicepipe

Narrated Audio Program

## What is voicepipe?

voicepipe is a Radio Narration Rendering pipeline.

It transforms generated radio scripts into narrated audio programs by combining text-to-speech synthesis and audio rendering.

## Current Goal

The primary recording workflow is:

```txt
upstream API or local Episode JSON -> VOICEVOX section WAVs -> ffmpeg concat -> MP3
```

VOICEVOX Engine is treated as an external TTS backend. voicepipe does not bundle VOICEVOX Engine, voice libraries, models, or Docker images.

voicepipe still does not implement upload APIs, S3 storage, result JSON submission, multiple TTS providers, BGM/SE mixing, volume normalization, cache-based regeneration skipping, or GUI features.

## Phase 2 Goal

Phase 2 adds local configuration and operational inspection commands while keeping rendering local-only:

- `voicepipe.toml` configuration loading
- CLI > config file > built-in default precedence
- `voicepipe speakers` for VOICEVOX speaker/style inspection
- `voicepipe doctor` for local environment validation

## Phase 3 Goal

Phase 3 adds `voicepipe preview` for faster voice tuning. Preview renders only a few short sections from an Episode JSON, so speaker and voice parameters can be adjusted without waiting for a full episode render.

## Requirements

- Rust toolchain
- Docker
- VOICEVOX Engine running locally or via Docker
- ffmpeg and ffprobe available from `PATH`

Default VOICEVOX endpoint is `http://127.0.0.1:50021`.

## Usage

Build and test with Makefile:

```bash
make build
make test
make clippy
make audit
```

Create local overrides from the example when needed:

```bash
cp voicepipe.sample.toml voicepipe.override.toml
```

Start a local VOICEVOX Engine container, record the sample episode, then stop the container:

```bash
make voicevox-up
make preview
make run
make voicevox-down
```

`make run` records from `samples/episode.json` and writes `dist/record/episode.mp3` by default.
`make preview` writes `dist/preview/preview.mp3` by default. It shares `INPUT` with `make run`, and uses `PREVIEW_OUTPUT` and `PREVIEW_WORKDIR` for preview-specific paths.

Override paths when needed:

```bash
make run \
  INPUT=./episode.json \
  OUTPUT=./dist/record/episode.mp3 \
  WORKDIR=./work/record/episode
```

For preview output separate from full render output:

```bash
make preview \
  PREVIEW_OUTPUT=./dist/preview/preview.mp3 \
  PREVIEW_WORKDIR=./work/preview
```

The underlying CLI command is:

```bash
cargo run -- record \
  --source json \
  --input ./samples/episode.json \
  --output ./dist/record/episode.mp3 \
  --workdir ./work/record/episode
```

The command reads `episode.scenario_json.sections[]`, synthesizes each section into a WAV file under the work directory, writes `concat.ffconcat` and `combined.wav`, then encodes the final MP3 with ffmpeg.

Record from an upstream API and save the exact Episode JSON used for recording:

```bash
cargo run -- record \
  --source upstream \
  --url https://example.com/api/episodes/latest \
  --output dist/record/episode.mp3 \
  --output-json dist/json/episode.json
```

Override the upstream URL from the command line:

```bash
cargo run -- record \
  --source upstream \
  --url https://example.com/api/episodes/latest \
  --output dist/record/episode.mp3 \
  --output-json dist/json/episode.json
```

Run the full upstream-to-downstream workflow:

```bash
cargo run -- onair
```

Limit processing to one discovered episode:

```bash
cargo run -- onair --limit 1
```

Preview discovery without download, recording, upload, or ledger writes:

```bash
cargo run -- onair --dry-run
```

Generate a short preview for tuning:

```bash
cargo run -- preview \
  --config ./voicepipe.sample.toml \
  --input ./samples/episode.json \
  --output ./dist/preview/preview.mp3 \
  --speaker 8 \
  --speed-scale 1.2 \
  --pitch-scale 0.05 \
  --intonation-scale 1.0 \
  --pause-length-scale 1.2
```

If `--output` is omitted, preview writes to a generated file under `dist/preview/`, such as:

```txt
dist/preview/preview_speaker8_speed120_pitch005_intonation100_pause120.mp3
```

## Configuration

The committed template is `voicepipe.sample.toml`. `voicepipe.toml`, `voicepipe.dist.toml`, and `voicepipe.override.toml` are ignored by git. Because the default stack reads `voicepipe.dist.toml` after `voicepipe.toml`, put local overrides in `voicepipe.override.toml` or pass an explicit file with `--config`.

```toml
[render]
input = "samples/episode.json"
output = "dist/record/episode.mp3"
workdir = "work/record/episode"

[preview]
input = "samples/episode.json"
output = "dist/preview/preview.mp3"
workdir = "work/preview"

[upstream]
episode_url = "https://example.com/api/episodes"
# access_token = "replace-with-local-token-or-use-env"

[downstream]
upload_url = "https://example.com/api/episodes"
# access_token = "replace-with-local-token-or-use-env"

[onair]
database = "dist/onair/onair.sqlite"
episodes_dir = "dist/onair/episodes"
work_dir = "work/onair"

[storage]
root_dir = "dist"
json_dir = "dist/json"
audio_dir = "dist/record"
preview_dir = "dist/preview"

[voicevox]
endpoint = "http://127.0.0.1:50021"
speaker = 3

[voice]
speed_scale = 1.2
pitch_scale = 0.0
intonation_scale = 0.9
pause_length_scale = 1.3
volume_scale = 1.0

[audio]
bitrate = "192k"
format = "mp3"
```

The `[storage]` section name is kept for configuration compatibility. Its default paths now point to `dist/`; new generated files should not default to `storage/`.

Configuration precedence:

```txt
CLI options
  ↓
voicepipe.override.toml
  ↓
voicepipe.dist.toml
  ↓
voicepipe.toml
  ↓
Built-in defaults
```

If `--config` is omitted, voicepipe loads existing files in this order:

```txt
voicepipe.toml
voicepipe.dist.toml
voicepipe.override.toml
```

Later files override earlier files. If none of these files exists, voicepipe exits with an error instead of silently using only built-in defaults.

If `--config` is specified, voicepipe loads only that file and ignores `voicepipe.toml`, `voicepipe.dist.toml`, and `voicepipe.override.toml`.

Upstream access tokens are resolved in this order:

```txt
VOICEPIPE_UPSTREAM_ACCESS_TOKEN
  ↓
[upstream].access_token
```

When a token is configured, `record --source upstream` and `onair` send it as an HTTP Bearer token to upstream. The token is not printed in logs.

Downstream upload access tokens are resolved in this order:

```txt
VOICEPIPE_DOWNSTREAM_ACCESS_TOKEN
  ↓
[downstream].access_token
```

When a downstream token is configured, `onair` sends it as an HTTP Bearer token on the upload `POST [downstream].upload_url` request. The token is not printed in logs.

For `onair`, `[upstream].episode_url` should point to the episode index endpoint, such as `/api/episodes`. For standalone `record --source upstream`, pass `--url` when you want to use a detail or latest endpoint such as `/api/episodes/latest`.

## Onair

`onair` orchestrates the full local processing workflow:

```txt
upstream -> Episode JSON -> record MP3 -> ffprobe duration -> downstream upload -> SQLite ledger
```

It uses:

- `GET [upstream].episode_url` to discover completed episodes
- `GET [upstream].episode_url/{episode_key}` to download each Episode JSON
- `dist/onair/episodes/{episode_key}/episode.json` for local JSON persistence
- `dist/onair/episodes/{episode_key}/audio.mp3` for recorded audio
- `dist/onair/episodes/{episode_key}/render_metadata.json` for render metadata
- `work/onair/{episode_key}/` for intermediate WAV and ffmpeg files
- `ffprobe` to extract `audio_duration_seconds` from the generated MP3
- `ffprobe` to measure each generated section WAV and replace `episode.scenario_json.sections[].estimated_duration_seconds` before upload
- `POST [downstream].upload_url` to upload audio, Episode JSON, render metadata, `recorded_at`, and `audio_duration_seconds`
- `dist/onair/onair.sqlite` to track processing state

The downstream upload multipart request includes `audio`, `episode_json`, `render_metadata_json`, `recorded_at`, and `audio_duration_seconds`.
The uploaded `episode_json` is a generated upload copy. The original downloaded `dist/onair/episodes/{episode_key}/episode.json` remains unchanged.

Basic usage:

```bash
cargo run -- onair
```

Process only the first discovered unprocessed episode:

```bash
cargo run -- onair --limit 1
```

Discovery only:

```bash
cargo run -- onair --dry-run
```

Before upload, `onair` replaces each `episode.scenario_json.sections[].estimated_duration_seconds` value with the measured duration of that section's generated WAV file, rounded to the nearest whole second. If a section WAV duration cannot be measured, the original upstream value is kept for that section and upload continues.

After MP3 generation succeeds, `onair` records `recorded_at` as a UTC RFC3339 timestamp. This is the render completion time, not the upload completion time. `audio_duration_seconds` is the total generated MP3 duration rounded to the nearest whole second. This is different from per-section `estimated_duration_seconds`.

The SQLite ledger table is `episodes`. It stores `recorded_at`, `audio_duration_seconds`, and `uploaded_at` separately. Uploaded episodes are considered processed and skipped on later runs. Failures are stored with `status = failed` and an `error_message`, and processing continues with the remaining episodes.

Default `onair` output layout:

```txt
dist/
  onair/
    onair.sqlite
    episodes/
      {episode_key}/
        episode.json
        audio.mp3
        render_metadata.json
```

Intermediate files for `onair` are written under `work/onair/{episode_key}/`.
Older local generated files under `storage/` are not migrated automatically and can be removed manually if they are no longer needed.

## Record

`record` is the main command for generating a full MP3 audio program.

Local JSON input:

```bash
cargo run -- record \
  --source json \
  --input samples/episode.json \
  --output dist/record/episode.mp3
```

Upstream API input:

```bash
cargo run -- record \
  --source upstream \
  --url https://example.com/api/episodes/latest \
  --output dist/record/episode.mp3 \
  --output-json dist/json/episode.json
```

Upstream URL override:

```bash
cargo run -- record \
  --source upstream \
  --url https://example.com/api/episodes/latest \
  --output dist/record/episode.mp3 \
  --output-json dist/json/episode.json
```

Record options:

- `--config`: configuration file path. If omitted, the default configuration stack is used.
- `--source`: required source selector. Supported values are `json` and `upstream`.
- `--input`: input Episode JSON file path. Required with `--source json`; invalid with `--source upstream`.
- `--url`: upstream Episode JSON URL. Valid only with `--source upstream`; overrides `[upstream].episode_url`.
- `--output`: output MP3 path. Defaults to `dist/record/{episode_key}.mp3`.
- `--output-json`: optional path to save the Episode JSON used for recording.
- `--workdir`: working directory for section WAV files and ffmpeg intermediates. Defaults to `work/record/<episode_key>`.
- `--voicevox-endpoint`: VOICEVOX Engine endpoint. Defaults to `http://127.0.0.1:50021`.
- `--speaker`: VOICEVOX speaker/style ID. Defaults to `3`.
- `--speed-scale`: VOICEVOX `speedScale`. Defaults to `1.2`.
- `--pitch-scale`: VOICEVOX `pitchScale`. Defaults to `0.0`.
- `--intonation-scale`: VOICEVOX `intonationScale`. Defaults to `0.9`.
- `--pause-length-scale`: VOICEVOX `pauseLengthScale`. Defaults to `1.3`.
- `--volume-scale`: VOICEVOX `volumeScale`. Defaults to `1.0`.

`render` remains available as a compatibility command for local JSON rendering. New workflows should use `record --source json`.

## Preview

`preview` uses the same Episode JSON, configuration, and voice options as `render`, but it selects a small subset of sections and trims each selected section before synthesis.

Preview supports three input modes. Use exactly one of them:

```bash
cargo run -- preview --input samples/episode.json
cargo run -- preview --text "これは読み上げテストです。"
echo "これは読み上げテストです。" | cargo run -- preview --stdin
```

`--text` and `--stdin` do not require Episode JSON. They create a temporary single-section preview internally and use the same synthesis and ffmpeg pipeline as Episode JSON preview.

Default section selection:

1. First `opening` section
2. First `topic` section
3. First `closing` section

Missing section types are skipped. Preview succeeds as long as at least one section is selected.

Additional preview options:

- `--max-sections`: maximum number of selected preview sections. Defaults to `3`.
- `--max-chars-per-section`: maximum text characters per section. Defaults to `300`.
- `--workdir`: preview work directory. Overrides `[preview].workdir`; defaults to `work/preview` when neither is set.

Suggested tuning workflow:

```bash
make voicevox-up

cargo run -- preview --input samples/episode.json --speaker 8 --speed-scale 1.1 --pitch-scale 0.00
cargo run -- preview --input samples/episode.json --speaker 8 --speed-scale 1.2 --pitch-scale 0.05
cargo run -- preview --input samples/episode.json --speaker 8 --speed-scale 1.3 --pitch-scale 0.08
```

Listen to the generated MP3 files, then copy the chosen values into `voicepipe.override.toml` or an explicit config file passed with `--config`.

## Inspection Commands

List VOICEVOX speakers and styles:

```bash
cargo run -- speakers --config ./voicepipe.sample.toml
```

Validate local prerequisites:

```bash
cargo run -- doctor --config ./voicepipe.sample.toml
```

`doctor` checks configuration validity, VOICEVOX reachability, ffmpeg availability, and writability of `dist` and `work` by default.

## Makefile Targets

- `make build`: build the Rust binary
- `make run`: record `samples/episode.json` into `dist/record/episode.mp3`
- `make onair`: run the upstream-to-downstream orchestration workflow
- `make preview`: render a short preview with `INPUT`, `PREVIEW_OUTPUT`, and `PREVIEW_WORKDIR`
- `make speakers`: list VOICEVOX speakers and styles
- `make doctor`: validate local prerequisites
- `make test`: run Rust tests
- `make fmt`: format Rust code
- `make fmt-check`: check Rust formatting
- `make clippy`: run clippy for all targets and features with warnings denied
- `make audit`: run `cargo audit`
- `make clean`: remove Cargo build artifacts plus generated files under `dist/` and `work/`
- `make check`: run `fmt-check`, `test`, `clippy`, and `audit`
- `make voicevox-up`: start local VOICEVOX Engine with Docker
- `make voicevox-down`: stop the local VOICEVOX Engine container
- `make voicevox-logs`: follow VOICEVOX Engine container logs
- `make voicevox-status`: check the VOICEVOX Engine version endpoint

## Input JSON

voicepipe consumes the rendering subset of the radiopipe episode export:

```json
{
  "episode": {
    "episode_key": "episode-001",
    "title": "Example Episode",
    "language": "ja",
    "scenario_json": {
      "sections": [
        {
          "type": "opening",
          "title": "オープニング",
          "text": "こんにちは。今日のニュースをお届けします。",
          "estimated_duration_seconds": 8
        }
      ]
    }
  }
}
```

Validation rules:

- `episode.language` must be `ja`
- `episode.scenario_json.sections` must contain at least one section
- `sections[].text` must not be empty
- `sections[].estimated_duration_seconds` is optional in Phase 1

The JSON Schema for the consumed subset is maintained at `docs/radiopipe-episode.schema.json`.
