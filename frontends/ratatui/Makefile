# Build and run Lem with the Ratatui frontend.
#
#   make          build both halves and the ./lem-ratatui launcher
#   make run      launch it against a scratch LEM_HOME
#   make dist     one self-contained binary, dist/lem-ratatui
#   make test     Rust unit tests
#   make clean    remove everything this file builds
#
# Works from any directory: `make -C frontends/ratatui` from the repo root
# is the same as `make` here.

HERE := $(abspath $(dir $(lastword $(MAKEFILE_LIST))))
ROOT := $(abspath $(HERE)/../..)

LISP ?= sbcl --dynamic-space-size 4GiB --noinform --no-sysinit --no-userinit --non-interactive
CARGO ?= cargo
LEM_HOME ?= /tmp/lem-scratch/

LISP_BIN := $(HERE)/lem-ratatui-lisp
RUST_BIN := $(HERE)/rust/target/release/lem-ratatui
LAUNCHER := $(HERE)/lem-ratatui
DIST_BIN := $(HERE)/dist/lem-ratatui
QLOT_SETUP := $(ROOT)/.qlot/setup.lisp

# The Lisp half is a saved image, so any Lisp source that ends up in it
# must trigger a rebuild, not just this frontend's own files.
LISP_SOURCES := $(HERE)/build.lisp $(HERE)/lem-ratatui.asd \
	$(shell find $(HERE)/lisp $(ROOT)/src $(ROOT)/extensions $(ROOT)/frontends/server \
		-name '*.lisp' -o -name '*.asd')

.PHONY: all lisp rust dist run test clean

all: $(LAUNCHER)

lisp: $(LISP_BIN)

rust: $(RUST_BIN)

$(QLOT_SETUP): $(ROOT)/qlfile.lock
	cd $(ROOT) && qlot install
	touch $@

$(LISP_BIN): $(QLOT_SETUP) $(LISP_SOURCES)
	cd $(ROOT) && $(LISP) --load .qlot/setup.lisp --load frontends/ratatui/build.lisp

# cargo tracks its own inputs, so always hand over to it.
$(RUST_BIN): FORCE
	cd $(HERE)/rust && $(CARGO) build --release

# Resolves its own location, so it can be symlinked onto PATH.
$(LAUNCHER): $(LISP_BIN) $(RUST_BIN)
	printf '%s\n' '#!/bin/sh' \
		'here=$$(dirname "$$(readlink -f "$$0")")' \
		'exec "$$here/rust/target/release/lem-ratatui" "$$here/lem-ratatui-lisp"' > $@
	chmod +x $@

# The Lisp image is embedded (zstd) and unpacked to ~/.cache/lem-ratatui/
# on first launch; see rust/crates/lem-ratatui/src/bundle.rs.
dist: $(DIST_BIN)

$(DIST_BIN): $(LISP_BIN) FORCE
	cd $(HERE)/rust && LEM_RATATUI_LISP=$(LISP_BIN) $(CARGO) build --profile dist --features bundle
	mkdir -p $(dir $@)
	cp $(HERE)/rust/target/dist/lem-ratatui $@

run: all
	mkdir -p $(LEM_HOME)
	LEM_HOME=$(LEM_HOME) $(LAUNCHER)

test:
	cd $(HERE)/rust && $(CARGO) test

clean:
	rm -rf $(LISP_BIN) $(LAUNCHER) $(dir $(DIST_BIN))
	cd $(HERE)/rust && $(CARGO) clean

FORCE:
