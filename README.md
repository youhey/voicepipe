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

## Phase 1 Goal

Phase 1 focuses on the smallest local rendering path:

```txt
local Episode JSON -> VOICEVOX section WAVs -> ffmpeg concat -> MP3
```

VOICEVOX Engine is treated as an external TTS backend. voicepipe does not bundle VOICEVOX Engine, voice libraries, models, or Docker images.

Phase 1 does not implement radiopipe API download, upload APIs, S3 storage, result JSON submission, configuration files, multiple TTS providers, BGM/SE mixing, volume normalization, cache-based regeneration skipping, or GUI features.

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
- ffmpeg available from `PATH`

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

Start a local VOICEVOX Engine container, render the sample episode, then stop the container:

```bash
make voicevox-up
make preview
make run
make voicevox-down
```

`make run` uses the default configuration stack, `samples/episode.json`, and writes `dist/episode.mp3` by default.
`make preview` uses the same `INPUT`, `OUTPUT`, and `WORKDIR` variables as `make run`.

Override paths when needed:

```bash
make run \
  INPUT=./episode.json \
  OUTPUT=./dist/episode.mp3 \
  WORKDIR=./work/episode
```

For preview output separate from full render output:

```bash
make preview \
  OUTPUT=./dist/preview.mp3 \
  WORKDIR=./work/preview
```

The underlying CLI command is:

```bash
cargo run -- render \
  --input ./samples/episode.json \
  --output ./dist/episode.mp3 \
  --workdir ./work/episode
```

The command reads `episode.scenario_json.sections[]`, synthesizes each section into a WAV file under the work directory, writes `concat.ffconcat` and `combined.wav`, then encodes the final MP3 with ffmpeg.

Generate a short preview for tuning:

```bash
cargo run -- preview \
  --config ./voicepipe.sample.toml \
  --input ./samples/episode.json \
  --output ./dist/preview.mp3 \
  --speaker 8 \
  --speed-scale 1.2 \
  --pitch-scale 0.05 \
  --intonation-scale 1.0 \
  --pause-length-scale 1.2
```

If `--output` is omitted, preview writes to a generated file under `dist/`, such as:

```txt
dist/preview_speaker8_speed120_pitch005_intonation100_pause120.mp3
```

## Configuration

The committed template is `voicepipe.sample.toml`. `voicepipe.toml`, `voicepipe.dist.toml`, and `voicepipe.override.toml` are ignored by git. Because the default stack reads `voicepipe.dist.toml` after `voicepipe.toml`, put local overrides in `voicepipe.override.toml` or pass an explicit file with `--config`.

```toml
[render]
input = "samples/episode.json"
output = "dist/episode.mp3"
workdir = "work/episode"

[preview]
input = "samples/episode.json"
output = "dist/preview.mp3"
workdir = "work/preview"

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

## Render Options

- `--config`: configuration file path. If omitted, the default configuration stack is used.
- `--input`: input Episode JSON file path. Overrides `[render].input`.
- `--output`: output MP3 file path. Overrides `[render].output`.
- `--workdir`: working directory for section WAV files and ffmpeg intermediates. Overrides `[render].workdir`; defaults to `./work/<episode_key>` when neither is set.
- `--voicevox-endpoint`: VOICEVOX Engine endpoint. Defaults to `http://127.0.0.1:50021`.
- `--speaker`: VOICEVOX speaker/style ID. Defaults to `3`.
- `--speed-scale`: VOICEVOX `speedScale`. Defaults to `1.2`.
- `--pitch-scale`: VOICEVOX `pitchScale`. Defaults to `0.0`.
- `--intonation-scale`: VOICEVOX `intonationScale`. Defaults to `0.9`.
- `--pause-length-scale`: VOICEVOX `pauseLengthScale`. Defaults to `1.3`.
- `--volume-scale`: VOICEVOX `volumeScale`. Defaults to `1.0`.

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
- `make run`: render `samples/episode.json` into `dist/episode.mp3`
- `make preview`: render a short preview with `INPUT`, `OUTPUT`, and `WORKDIR`
- `make speakers`: list VOICEVOX speakers and styles
- `make doctor`: validate local prerequisites
- `make test`: run Rust tests
- `make fmt`: format Rust code
- `make fmt-check`: check Rust formatting
- `make clippy`: run clippy for all targets and features with warnings denied
- `make audit`: run `cargo audit`
- `make check`: run `fmt-check`, `test`, `clippy`, and `audit`
- `make clean`: remove Cargo build artifacts
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
