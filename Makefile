VOICEVOX_CONTAINER := voicepipe-voicevox
VOICEVOX_IMAGE := voicevox/voicevox_engine:cpu-latest
VOICEVOX_PORT ?= 50021
VOICEVOX_ENDPOINT ?= http://127.0.0.1:$(VOICEVOX_PORT)

CONFIG ?= voicepipe.example.toml
INPUT ?= samples/episode.json
OUTPUT ?= dist/episode.mp3
PREVIEW_OUTPUT ?= dist/preview.mp3
WORKDIR ?= work/episode
PREVIEW_WORKDIR ?= work/preview
SPEAKER ?= 3
SPEED_SCALE ?= 1.2
PITCH_SCALE ?= 0.0
INTONATION_SCALE ?= 0.9
PAUSE_LENGTH_SCALE ?= 1.3
VOLUME_SCALE ?= 1.0

.PHONY: build run preview speakers doctor test audit fmt fmt-check clippy clean check
.PHONY: voicevox-up voicevox-down voicevox-logs voicevox-status

build:
	cargo build

run:
	cargo run -- render \
		--config $(CONFIG) \
		--input $(INPUT) \
		--output $(OUTPUT) \
		--workdir $(WORKDIR) \
		--voicevox-endpoint $(VOICEVOX_ENDPOINT) \
		--speaker $(SPEAKER) \
		--speed-scale $(SPEED_SCALE) \
		--pitch-scale $(PITCH_SCALE) \
		--intonation-scale $(INTONATION_SCALE) \
		--pause-length-scale $(PAUSE_LENGTH_SCALE) \
		--volume-scale $(VOLUME_SCALE)

preview:
	cargo run -- preview \
		--config $(CONFIG) \
		--input $(INPUT) \
		--output $(PREVIEW_OUTPUT) \
		--workdir $(PREVIEW_WORKDIR) \
		--voicevox-endpoint $(VOICEVOX_ENDPOINT) \
		--speaker $(SPEAKER) \
		--speed-scale $(SPEED_SCALE) \
		--pitch-scale $(PITCH_SCALE) \
		--intonation-scale $(INTONATION_SCALE) \
		--pause-length-scale $(PAUSE_LENGTH_SCALE) \
		--volume-scale $(VOLUME_SCALE)

speakers:
	cargo run -- speakers --config $(CONFIG)

doctor:
	cargo run -- doctor --config $(CONFIG)

test:
	cargo test

audit:
	cargo audit

fmt:
	cargo fmt

fmt-check:
	cargo fmt --check

clippy:
	cargo clippy --all-targets --all-features -- -D warnings

check: fmt-check test clippy audit

clean:
	cargo clean

voicevox-up:
	@if docker ps --filter "name=^/$(VOICEVOX_CONTAINER)$$" --format "{{.Names}}" | grep -qx "$(VOICEVOX_CONTAINER)"; then \
		echo "$(VOICEVOX_CONTAINER) is already running"; \
	else \
		docker run --rm -d \
			--name $(VOICEVOX_CONTAINER) \
			-p $(VOICEVOX_PORT):50021 \
			$(VOICEVOX_IMAGE); \
	fi

voicevox-down:
	docker stop $(VOICEVOX_CONTAINER)

voicevox-logs:
	docker logs -f $(VOICEVOX_CONTAINER)

voicevox-status:
	curl -fsS $(VOICEVOX_ENDPOINT)/version
