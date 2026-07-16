CARGO ?= cargo
INFERNO_FLAMEGRAPH ?= inferno-flamegraph
LOG_DIR ?= logs

.PHONY: help build test fmt lint run flame flame-idle run-flame clean-flames

help:
	@printf '%s\n' \
	  'VoidMC development commands:' \
	  '  make build        Build every workspace crate.' \
	  '  make test         Run the workspace test suite.' \
	  '  make fmt          Format Rust sources.' \
	  '  make lint         Run Clippy with warnings denied.' \
	  '  make run          Run the release example server.' \
	  '  make flame        Profile active work; Ctrl-C renders both SVG variants.' \
	  '  make flame-idle   Profile with idle time; Ctrl-C renders both SVG variants.' \
	  '  make clean-flames Remove generated flame profiling artifacts.'

build:
	$(CARGO) build --workspace

test:
	$(CARGO) test --workspace

fmt:
	$(CARGO) fmt --all

lint:
	$(CARGO) clippy --all-targets --all-features -- -D warnings

run:
	$(CARGO) run --release -p voidmc-example

# The default profiling view: idle gaps are omitted, so application hotspots
# occupy the full aggregate flamegraph.
flame:
	@$(MAKE) --no-print-directory run-flame PROFILE=active

# Includes scheduler idle gaps, allowing the aggregate graph to show overall
# server utilization as well as application work.
flame-idle:
	@$(MAKE) --no-print-directory run-flame PROFILE=idle INCLUDE_IDLE=1

# Internal target used by `flame` and `flame-idle`. It renders the aggregate
# flamegraph and the chronological flamechart after the server exits. The INT
# and TERM traps also render the trace when the profiling run is stopped with
# Ctrl-C.
run-flame:
	@if test -z "$(PROFILE)"; then echo 'PROFILE is required'; exit 2; fi
	@if ! command -v "$(INFERNO_FLAMEGRAPH)" >/dev/null 2>&1; then \
	  echo 'inferno-flamegraph is required; install it with: cargo install inferno'; \
	  exit 2; \
	fi
	@mkdir -p "$(LOG_DIR)"
	@folded="$(LOG_DIR)/void-flame-$(PROFILE).folded"; \
	aggregate="$(LOG_DIR)/void-flame-$(PROFILE).svg"; \
	timeline="$(LOG_DIR)/void-flame-$(PROFILE)-timeline.svg"; \
	render() { \
	  if test ! -s "$$folded"; then \
	    echo "No folded trace was written; skipping SVG rendering."; \
	    return; \
	  fi; \
	  "$(INFERNO_FLAMEGRAPH)" --title "VoidMC $(PROFILE) profile" "$$folded" > "$$aggregate"; \
	  "$(INFERNO_FLAMEGRAPH)" --flamechart --title "VoidMC $(PROFILE) timeline" "$$folded" > "$$timeline"; \
	  echo "Wrote $$aggregate and $$timeline"; \
	}; \
	trap 'render || exit $$?; exit 0' INT TERM; \
	env VOID_METRICS_MODE=flame VOID_FLAME_OUTPUT="$$folded" $(if $(INCLUDE_IDLE),VOID_FLAME_INCLUDE_IDLE=1) \
	  $(CARGO) run --release -p voidmc-example; \
	status=$$?; \
	render || exit $$?; \
	exit $$status

clean-flames:
	@find "$(LOG_DIR)" -maxdepth 1 -type f \( \
	  -name 'void-flame-*.folded' -o -name 'void-flame-*.svg' \
	\) -delete
