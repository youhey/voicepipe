VOICEVOX_CONTAINER := voicepipe-voicevox
VOICEVOX_IMAGE := voicevox/voicevox_engine:cpu-latest
VOICEVOX_PORT ?= 50021
VOICEVOX_ENDPOINT ?= http://127.0.0.1:$(VOICEVOX_PORT)

INPUT ?= samples/episode.json
OUTPUT ?= dist/episode.mp3
WORKDIR ?= work/episode

.PHONY: build run preview speakers doctor test audit fmt fmt-check clippy clean check
.PHONY: voicevox-up voicevox-down voicevox-logs voicevox-status

build:
	cargo build

run:
	cargo run -- record \
		--source json \
		--input $(INPUT) \
		--output $(OUTPUT) \
		--workdir $(WORKDIR)

preview:
	cargo run -- preview \
		--input $(INPUT) \
		--output $(OUTPUT) \
		--workdir $(WORKDIR)

speakers:
	cargo run -- speakers

doctor:
	cargo run -- doctor

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
