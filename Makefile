VOICEVOX_CONTAINER := voicepipe-voicevox
VOICEVOX_IMAGE := voicevox/voicevox_engine:cpu-latest
VOICEVOX_PORT ?= 50021
VOICEVOX_ENDPOINT ?= http://127.0.0.1:$(VOICEVOX_PORT)

INPUT ?= samples/episode.json
OUTPUT ?= dist/episode.mp3
WORKDIR ?= work/episode
SPEAKER ?= 3

.PHONY: build run test audit fmt fmt-check clippy clean check
.PHONY: voicevox-up voicevox-down voicevox-logs voicevox-status

build:
	cargo build

run:
	cargo run -- render \
		--input $(INPUT) \
		--output $(OUTPUT) \
		--workdir $(WORKDIR) \
		--voicevox-endpoint $(VOICEVOX_ENDPOINT) \
		--speaker $(SPEAKER)

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
	docker run --rm -d \
		--name $(VOICEVOX_CONTAINER) \
		-p $(VOICEVOX_PORT):50021 \
		$(VOICEVOX_IMAGE)

voicevox-down:
	docker stop $(VOICEVOX_CONTAINER)

voicevox-logs:
	docker logs -f $(VOICEVOX_CONTAINER)

voicevox-status:
	curl -fsS $(VOICEVOX_ENDPOINT)/version
