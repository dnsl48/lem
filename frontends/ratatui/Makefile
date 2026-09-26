# Build and run Lem with the Ratatui frontend.
#
#   make          build everything and the dist/lem-ratatui-dev script
#   make run      launch it against a scratch LEM_HOME
#   make dist     one self-contained binary, dist/lem-ratatui
#   make test     Rust and Lisp unit tests (Lisp needs Roswell for rove)
#   make toolchain  protoc and cl-protobufs' plugin, built once into
#                 toolchain/ (a C++17 compiler, CMake, zlib and git)
#   make clean    remove everything this file builds, except toolchain/
#   make distclean  and toolchain/ too
#
# Works from any directory: `make -C frontends/ratatui` from the repo root
# is the same as `make` here.

HERE := $(abspath $(dir $(lastword $(MAKEFILE_LIST))))
ROOT := $(abspath $(HERE)/../..)

LISP ?= sbcl --dynamic-space-size 4GiB --noinform --no-sysinit --no-userinit --non-interactive
CARGO ?= cargo
LEM_HOME ?= /tmp/lem-scratch/

# Every build output lands in dist/.
LISP_BIN := $(HERE)/dist/lem-ratatui-lisp
# The launcher finds the display binary beside itself in target/release.
RUST_BIN := $(HERE)/rust/target/release/lem-ratatui-launcher
LAUNCHER := $(HERE)/dist/lem-ratatui-dev
DIST_BIN := $(HERE)/dist/lem-ratatui
QLOT_SETUP := $(ROOT)/.qlot/setup.lisp

# The protobuf toolchain (docs/adr/0010, docs/protobuf-spike.md), pinned
# and built here rather than taken from distro packages. It lives in
# toolchain/, which is gitignored:
#
#   toolchain/cl-protobufs/  the Lisp runtime; qlot's project searcher finds
#                            its .asd there, so the root qlfile is untouched
#   toolchain/bin/           protoc and protoc-gen-cl-pb, put first on PATH
#                            for every Lisp build: cl-protobufs compiles
#                            .proto files, its own included, as it loads
#   toolchain/.build/        sources, build trees and the protobuf install.
#                            Dot-named because qlot's searcher skips those,
#                            and would otherwise walk every header on each
#                            system lookup.
#
# PROTOC_PREFIX=/usr/local (say) uses an installed protobuf, which needs
# its headers and CMake files as well as protoc, instead of building one.
PROTOBUF_VERSION := v36.2
CL_PROTOBUFS_COMMIT := b15c26db728601e2dc5d3c0b7c3cf6fe5c526fac
PROTOC_PREFIX ?=
JOBS ?= $(shell nproc 2>/dev/null || echo 4)

TOOLCHAIN := $(HERE)/toolchain
TOOLCHAIN_BUILD := $(TOOLCHAIN)/.build
PROTOBUF_PREFIX := $(if $(PROTOC_PREFIX),$(PROTOC_PREFIX),$(TOOLCHAIN_BUILD)/prefix)
# Named after the pin, so changing a pin rebuilds what depends on it.
PROTOBUF_STAMP := $(TOOLCHAIN_BUILD)/protobuf-$(PROTOBUF_VERSION).stamp
CL_PROTOBUFS_STAMP := $(TOOLCHAIN_BUILD)/cl-protobufs-$(CL_PROTOBUFS_COMMIT).stamp
PROTOC := $(TOOLCHAIN)/bin/protoc
PROTOC_PLUGIN := $(TOOLCHAIN)/bin/protoc-gen-cl-pb
WITH_TOOLCHAIN := PATH="$(TOOLCHAIN)/bin:$$PATH"

# The Lisp half is a saved image, so any Lisp source that ends up in it
# must trigger a rebuild, not just this frontend's own files.
LISP_SOURCES := $(HERE)/build.lisp $(HERE)/lem-ratatui.asd \
	$(shell find $(HERE)/lisp $(HERE)/relay $(ROOT)/src $(ROOT)/extensions \
		-name '*.lisp' -o -name '*.asd')

.PHONY: all lisp rust dist run test test-rust test-lisp toolchain clean distclean

all: $(LAUNCHER)

lisp: $(LISP_BIN)

rust: $(RUST_BIN)

$(QLOT_SETUP): $(ROOT)/qlfile.lock
	cd $(ROOT) && qlot install
	touch $@

$(LISP_BIN): $(QLOT_SETUP) $(LISP_SOURCES) $(CL_PROTOBUFS_STAMP) | $(PROTOC) $(PROTOC_PLUGIN)
	cd $(ROOT) && $(WITH_TOOLCHAIN) $(LISP) --load .qlot/setup.lisp --load frontends/ratatui/build.lisp

# cargo tracks its own inputs, so always hand over to it.
$(RUST_BIN): FORCE
	cd $(HERE)/rust && $(CARGO) build --release

# Resolves its own location, so it can be symlinked onto PATH.
$(LAUNCHER): $(LISP_BIN) $(RUST_BIN)
	printf '%s\n' '#!/bin/sh' \
		'here=$$(dirname "$$(readlink -f "$$0")")' \
		'LEM_RATATUI_LISP="$$here/lem-ratatui-lisp" exec "$$here/../rust/target/release/lem-ratatui-launcher" "$$@"' > $@
	chmod +x $@

# The launcher, with the Lisp image and the display binary embedded (zstd)
# and unpacked to ~/.cache/lem-ratatui/ on first launch; see
# rust/crates/lem-ratatui-launcher/src/bundle.rs. The display binary is
# built first because the launcher's build script reads it.
dist: $(DIST_BIN)

$(DIST_BIN): $(LISP_BIN) FORCE
	cd $(HERE)/rust && $(CARGO) build --profile dist -p lem-ratatui
	cd $(HERE)/rust && LEM_RATATUI_LISP=$(LISP_BIN) LEM_RATATUI_TERMINAL=$(HERE)/rust/target/dist/lem-ratatui \
		$(CARGO) build --profile dist -p lem-ratatui-launcher --features bundle
	cp $(HERE)/rust/target/dist/lem-ratatui-launcher $@

run: all
	mkdir -p $(LEM_HOME)
	LEM_HOME=$(LEM_HOME) $(LAUNCHER)

test: test-rust test-lisp

test-rust:
	cd $(HERE)/rust && $(CARGO) test

test-lisp: $(QLOT_SETUP) $(CL_PROTOBUFS_STAMP) | $(PROTOC) $(PROTOC_PLUGIN)
	cd $(ROOT) && $(WITH_TOOLCHAIN) .qlot/bin/rove frontends/ratatui/relay/lem-relay.asd

toolchain: $(CL_PROTOBUFS_STAMP) $(PROTOC) $(PROTOC_PLUGIN)

$(CL_PROTOBUFS_STAMP):
	rm -rf $(TOOLCHAIN)/cl-protobufs $(TOOLCHAIN_BUILD)/cl-protobufs-*.stamp
	mkdir -p $(TOOLCHAIN_BUILD)
	git clone --quiet https://github.com/qitab/cl-protobufs.git $(TOOLCHAIN)/cl-protobufs
	git -C $(TOOLCHAIN)/cl-protobufs -c advice.detachedHead=false checkout --quiet $(CL_PROTOBUFS_COMMIT)
	touch $@

$(PROTOBUF_STAMP):
	rm -rf $(TOOLCHAIN_BUILD)/protobuf $(TOOLCHAIN_BUILD)/prefix $(TOOLCHAIN_BUILD)/protobuf-*.stamp
	mkdir -p $(TOOLCHAIN_BUILD)
	git -c advice.detachedHead=false clone --quiet --depth 1 --branch $(PROTOBUF_VERSION) \
		https://github.com/protocolbuffers/protobuf.git $(TOOLCHAIN_BUILD)/protobuf
	cmake -S $(TOOLCHAIN_BUILD)/protobuf -B $(TOOLCHAIN_BUILD)/protobuf/build \
		-DCMAKE_BUILD_TYPE=Release -DCMAKE_CXX_STANDARD=17 -Dprotobuf_BUILD_TESTS=OFF \
		-Dprotobuf_ABSL_PROVIDER=module -DABSL_PROPAGATE_CXX_STD=ON \
		-DCMAKE_INSTALL_PREFIX=$(TOOLCHAIN_BUILD)/prefix
	cmake --build $(TOOLCHAIN_BUILD)/protobuf/build --parallel $(JOBS)
	cmake --install $(TOOLCHAIN_BUILD)/protobuf/build
	touch $@

# A wrapper, not a symlink or a copy: make compares a symlink by what it
# points at, which predates the stamp, so it would never look built; and
# protoc finds its bundled .proto files relative to its real location,
# which a copy would lose.
$(PROTOC): $(if $(PROTOC_PREFIX),,$(PROTOBUF_STAMP))
	mkdir -p $(dir $@)
	printf '%s\n' '#!/bin/sh' 'exec "$(PROTOBUF_PREFIX)/bin/protoc" "$$@"' > $@
	chmod +x $@

# The plugin's CMake only builds in-source (docs/protobuf-spike.md), and
# reaches into the rest of the checkout, so it builds in a whole copy.
$(PROTOC_PLUGIN): $(CL_PROTOBUFS_STAMP) $(if $(PROTOC_PREFIX),,$(PROTOBUF_STAMP))
	rm -rf $(TOOLCHAIN_BUILD)/plugin
	cp -r $(TOOLCHAIN)/cl-protobufs $(TOOLCHAIN_BUILD)/plugin
	cd $(TOOLCHAIN_BUILD)/plugin/protoc && cmake . -DCMAKE_BUILD_TYPE=Release \
		-DCMAKE_CXX_STANDARD=17 -DCMAKE_PREFIX_PATH=$(PROTOBUF_PREFIX)
	cmake --build $(TOOLCHAIN_BUILD)/plugin/protoc --parallel $(JOBS)
	mkdir -p $(dir $@)
	cp $(TOOLCHAIN_BUILD)/plugin/protoc/protoc-gen-cl-pb $@

clean:
	rm -rf $(HERE)/dist
	cd $(HERE)/rust && $(CARGO) clean

distclean: clean
	rm -rf $(TOOLCHAIN)

FORCE:
