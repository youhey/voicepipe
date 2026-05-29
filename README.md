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

Create local configuration from the example when needed:

```bash
cp voicepipe.example.toml voicepipe.toml
```

Start a local VOICEVOX Engine container, render the sample episode, then stop the container:

```bash
make voicevox-up
make preview
make run
make voicevox-down
```

`make run` uses `voicepipe.example.toml`, `samples/episode.json`, and writes `dist/episode.mp3` by default.
`make preview` uses the same input and writes `dist/preview.mp3` by default.

Override paths, speaker, or the VOICEVOX endpoint when needed:

```bash
make run \
  INPUT=./episode.json \
  OUTPUT=./dist/episode.mp3 \
  WORKDIR=./work/episode \
  SPEAKER=3 \
  SPEED_SCALE=1.2 \
  PITCH_SCALE=0.0 \
  INTONATION_SCALE=0.9 \
  PAUSE_LENGTH_SCALE=1.3 \
  VOLUME_SCALE=1.0 \
  VOICEVOX_ENDPOINT=http://127.0.0.1:50021
```

The underlying CLI command is:

```bash
cargo run -- render \
  --config ./voicepipe.example.toml \
  --input ./samples/episode.json \
  --output ./dist/episode.mp3 \
  --workdir ./work/episode \
  --voicevox-endpoint http://127.0.0.1:50021 \
  --speaker 3 \
  --speed-scale 1.2 \
  --pitch-scale 0.0 \
  --intonation-scale 0.9 \
  --pause-length-scale 1.3 \
  --volume-scale 1.0
```

The command reads `episode.scenario_json.sections[]`, synthesizes each section into a WAV file under the work directory, writes `concat.ffconcat` and `combined.wav`, then encodes the final MP3 with ffmpeg.

Generate a short preview for tuning:

```bash
cargo run -- preview \
  --config ./voicepipe.example.toml \
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

The local config file is `voicepipe.toml`. It is ignored by git. Use `voicepipe.example.toml` as the committed template.

```toml
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
Config file
  ↓
Built-in defaults
```

If `--config` is omitted, voicepipe tries `./voicepipe.toml`. If it does not exist, built-in defaults are used.

## Render Options

- `--config`: configuration file path. If omitted, `./voicepipe.toml` is used when present.
- `--input`: input Episode JSON file path. Required.
- `--output`: output MP3 file path. Required.
- `--workdir`: working directory for section WAV files and ffmpeg intermediates. Defaults to `./work/<episode_key>`.
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
- `--workdir`: preview work directory. Defaults to `work/preview`.

Suggested tuning workflow:

```bash
make voicevox-up

cargo run -- preview --input samples/episode.json --speaker 8 --speed-scale 1.1 --pitch-scale 0.00
cargo run -- preview --input samples/episode.json --speaker 8 --speed-scale 1.2 --pitch-scale 0.05
cargo run -- preview --input samples/episode.json --speaker 8 --speed-scale 1.3 --pitch-scale 0.08
```

Listen to the generated MP3 files, then copy the chosen values into `voicepipe.toml`.

## Inspection Commands

List VOICEVOX speakers and styles:

```bash
cargo run -- speakers --config ./voicepipe.example.toml
```

Validate local prerequisites:

```bash
cargo run -- doctor --config ./voicepipe.example.toml
```

`doctor` checks configuration validity, VOICEVOX reachability, ffmpeg availability, and writability of `dist` and `work` by default.

## Makefile Targets

- `make build`: build the Rust binary
- `make run`: render `samples/episode.json` into `dist/episode.mp3`
- `make preview`: render a short preview into `dist/preview.mp3`
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
