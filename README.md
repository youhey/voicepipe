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

## Requirements

- Rust toolchain
- VOICEVOX Engine running locally
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

Start a local VOICEVOX Engine container, render the sample episode, then stop the container:

```bash
make voicevox-up
make run
make voicevox-down
```

`make run` uses `samples/episode.json` and writes `dist/episode.mp3` by default.

Override paths, speaker, or the VOICEVOX endpoint when needed:

```bash
make run \
  INPUT=./episode.json \
  OUTPUT=./dist/episode.mp3 \
  WORKDIR=./work/episode \
  SPEAKER=3 \
  VOICEVOX_ENDPOINT=http://127.0.0.1:50021
```

The underlying CLI command is:

```bash
cargo run -- render \
  --input ./samples/episode.json \
  --output ./dist/episode.mp3 \
  --workdir ./work/episode \
  --voicevox-endpoint http://127.0.0.1:50021 \
  --speaker 3
```

The command reads `episode.scenario_json.sections[]`, synthesizes each section into a WAV file under the work directory, writes `concat.ffconcat` and `combined.wav`, then encodes the final MP3 with ffmpeg.

## Makefile Targets

- `make build`: build the Rust binary
- `make run`: render `samples/episode.json` into `dist/episode.mp3`
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
          "text": "こんにちは。今日のニュースをお届けします。"
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

The JSON Schema for the consumed subset is maintained at `docs/radiopipe-episode.schema.json`.
