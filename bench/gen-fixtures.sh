#!/usr/bin/env bash
# Build the third-party tools the IO-throughput benchmark needs and materialise
# its fixtures. Everything lands under bench/.cache and bench/fixtures, both of
# which are gitignored.
#
#   bench/gen-fixtures.sh              # build what is missing
#   bench/gen-fixtures.sh --force      # regenerate fixtures even if present
#   bench/gen-fixtures.sh --extra      # also emit the unused osc/kitty fixtures
#
# See bench/README.md for the protocol these fixtures serve.
set -euo pipefail

BENCH_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
CACHE_DIR="$BENCH_DIR/.cache"
FIXTURE_DIR="$BENCH_DIR/fixtures"

# 150 MiB, byte-exact, matching the upstream benchmark this reproduces.
FIXTURE_BYTES=157286400
# Pinned so a regenerated unicode fixture is byte-identical.
UTF8_SEED=42

# Pinned upstream revisions. Override to re-pin without editing this file.
GHOSTTY_REPO="${GHOSTTY_REPO:-https://github.com/ghostty-org/ghostty}"
GHOSTTY_REF="${GHOSTTY_REF:-40ab02e3389fe9ff59c3ea682a48359c68ecaf4a}"
DOOM_REPO="${DOOM_REPO:-https://github.com/const-void/DOOM-fire-zig}"
DOOM_REF="${DOOM_REF:-eb0631b141b5778eefc6f5767bb45f8974c1be71}"

# ghostty needs the same Zig the zz workspace pins; DOOM-fire-zig is stuck on
# the pre-Writergate standard library.
GHOSTTY_ZIG="${GHOSTTY_ZIG:-0.16.0}"
DOOM_ZIG="${DOOM_ZIG:-0.14.1}"

FORCE=0
EXTRA=0
for arg in "$@"; do
	case "$arg" in
	--force) FORCE=1 ;;
	--extra) EXTRA=1 ;;
	-h | --help)
		sed -n '2,10p' "${BASH_SOURCE[0]}"
		exit 0
		;;
	*)
		echo "gen-fixtures: unknown argument: $arg" >&2
		exit 2
		;;
	esac
done

log() { printf '\033[1;36m==>\033[0m %s\n' "$*" >&2; }
warn() { printf '\033[1;33mwarn:\033[0m %s\n' "$*" >&2; }
die() {
	printf '\033[1;31merror:\033[0m %s\n' "$*" >&2
	exit 1
}

file_size() {
	stat -c %s "$1" 2>/dev/null || stat -f %z "$1"
}

sha256_of() {
	if command -v sha256sum >/dev/null 2>&1; then
		sha256sum "$1" | cut -d' ' -f1
	else
		shasum -a 256 "$1" | cut -d' ' -f1
	fi
}

zig_for() {
	local want="$1"
	if command -v zig >/dev/null 2>&1 && [ "$(zig version 2>/dev/null)" = "$want" ]; then
		printf 'zig'
		return 0
	fi
	if command -v mise >/dev/null 2>&1; then
		mise install "zig@$want" >/dev/null 2>&1 || true
		if mise x "zig@$want" -- zig version >/dev/null 2>&1; then
			printf 'mise x zig@%s -- zig' "$want"
			return 0
		fi
	fi
	return 1
}

# Full history with lazy blobs, so re-pinning to an older revision never needs
# a second clone.
clone_pinned() {
	local repo="$1" ref="$2" dest="$3"
	if [ ! -d "$dest/.git" ]; then
		log "cloning $repo"
		git clone --filter=blob:none "$repo" "$dest"
	fi
	if [ "$(git -C "$dest" rev-parse HEAD)" != "$ref" ]; then
		log "checking out ${ref:0:12} in $(basename "$dest")"
		git -C "$dest" fetch --filter=blob:none origin "$ref" 2>/dev/null ||
			git -C "$dest" fetch --filter=blob:none origin
		git -C "$dest" checkout --detach "$ref"
	fi
}

command -v git >/dev/null 2>&1 || die "git is required"
mkdir -p "$CACHE_DIR" "$FIXTURE_DIR"

GHOSTTY_DIR="$CACHE_DIR/ghostty"
GEN_BIN="$GHOSTTY_DIR/zig-out/bin/ghostty-gen"

clone_pinned "$GHOSTTY_REPO" "$GHOSTTY_REF" "$GHOSTTY_DIR"

if [ ! -x "$GEN_BIN" ]; then
	GHOSTTY_ZIG_CMD="$(zig_for "$GHOSTTY_ZIG")" ||
		die "zig $GHOSTTY_ZIG not found and mise could not provide it"
	log "building ghostty-gen with zig $GHOSTTY_ZIG (this takes a few minutes)"
	(
		cd "$GHOSTTY_DIR"
		# `-Dapp-runtime=none` keeps this to the generator: the default install
		# step also builds the terminal, which on Linux drags in GTK4/libadwaita.
		# shellcheck disable=SC2086 # GHOSTTY_ZIG_CMD is a deliberate word list
		$GHOSTTY_ZIG_CMD build -Demit-bench -Doptimize=ReleaseFast \
			-Demit-macos-app=false -Dapp-runtime=none
	)
fi
[ -x "$GEN_BIN" ] || die "ghostty-gen was not produced at $GEN_BIN"

# generate <output> <ghostty-gen args...>
generate() {
	local out="$1"
	shift
	if [ "$FORCE" -eq 0 ] && [ -f "$out" ] && [ "$(file_size "$out")" = "$FIXTURE_BYTES" ]; then
		log "$(basename "$out") already present ($FIXTURE_BYTES bytes)"
		return 0
	fi
	log "generating $(basename "$out") ($FIXTURE_BYTES bytes)"
	# ghostty-gen streams forever; head closes the pipe and the generator
	# treats BrokenPipe as a clean stop.
	"$GEN_BIN" "$@" 2>/dev/null | head -c "$FIXTURE_BYTES" >"$out.part" || true
	local got
	got="$(file_size "$out.part")"
	[ "$got" = "$FIXTURE_BYTES" ] || die "$(basename "$out"): got $got bytes, want $FIXTURE_BYTES"
	mv "$out.part" "$out"
}

# `+ascii` has no --seed upstream (its Options struct is empty), so it is
# time-seeded: the cached file plus its recorded sha256 keep one session
# self-consistent, but cross-machine byte equality is lost. `+utf8` is pinned.
generate "$FIXTURE_DIR/150MB_ascii.txt" +ascii
generate "$FIXTURE_DIR/150MB_unicode.txt" +utf8 "--seed=$UTF8_SEED"

if [ "$EXTRA" -eq 1 ]; then
	# Unused by any test; see "Out of scope" in bench/README.md.
	generate "$FIXTURE_DIR/150MB_osc.txt" +osc "--seed=$UTF8_SEED"
	generate "$FIXTURE_DIR/150MB_kitty.txt" +kitty "--seed=$UTF8_SEED"
fi

DOOM_DIR="$CACHE_DIR/DOOM-fire-zig"
DOOM_BIN="$DOOM_DIR/zig-out/bin/DOOM-fire"
DOOM_STATUS="unavailable"

build_doom() {
	local zigcmd
	zigcmd="$(zig_for "$DOOM_ZIG")" || {
		warn "zig $DOOM_ZIG unavailable; skipping DOOM-fire (the doom-fire test will be skipped)"
		return 1
	}

	# Upstream ends its intro demo with a blocking "Press return to continue".
	# Unattended runs cannot answer it, so drop the intro entirely; the fps
	# counter only covers the fire loop anyway (t_start is set in initBuf).
	if ! grep -q 'zz-bench' "$DOOM_DIR/src/main.zig"; then
		log "patching DOOM-fire-zig to skip its interactive intro"
		sed -i.zzbak \
			's|^    try showTermCap();$|    // zz-bench: skipped (interactive "Press return" prompt) -- try showTermCap();|' \
			"$DOOM_DIR/src/main.zig"
		rm -f "$DOOM_DIR/src/main.zig.zzbak"
		grep -q 'zz-bench' "$DOOM_DIR/src/main.zig" ||
			die "DOOM-fire patch did not apply; upstream source moved"
	fi

	mkdir -p "$DOOM_DIR/zig-out/bin"
	log "building DOOM-fire with zig $DOOM_ZIG"
	(
		cd "$DOOM_DIR"
		# shellcheck disable=SC2086
		if $zigcmd build -Doptimize=ReleaseFast >/dev/null 2>&1 && [ -x "$DOOM_BIN" ]; then
			exit 0
		fi
		# On macOS 27 with SDK 26.x, zig 0.14's build runner fails to link
		# libSystem for the native target ("undefined symbol: _free"). build-exe
		# with an explicit macOS version in the triple sidesteps the runner.
		local arch
		arch="$(uname -m)"
		[ "$arch" = "arm64" ] && arch="aarch64"
		local os="macos.14.0"
		[ "$(uname -s)" = "Linux" ] && os="linux-gnu"
		# shellcheck disable=SC2086
		$zigcmd build-exe src/main.zig -lc -OReleaseFast \
			-target "$arch-$os" --name DOOM-fire \
			-femit-bin="zig-out/bin/DOOM-fire" >/dev/null
		rm -f zig-out/bin/DOOM-fire.o
	)
	[ -x "$DOOM_BIN" ]
}

clone_pinned "$DOOM_REPO" "$DOOM_REF" "$DOOM_DIR"
if [ -x "$DOOM_BIN" ] && [ "$FORCE" -eq 0 ]; then
	DOOM_STATUS="cached"
elif build_doom; then
	DOOM_STATUS="built"
else
	warn "DOOM-fire is unavailable; cat-ascii and cat-unicode still work"
fi

{
	echo "# zz bench fixture provenance: regenerate with bench/gen-fixtures.sh"
	echo "generated_at        $(date -u +%Y-%m-%dT%H:%M:%SZ)"
	echo "host_os             $(uname -srm)"
	echo "ghostty_repo        $GHOSTTY_REPO"
	echo "ghostty_commit      $(git -C "$GHOSTTY_DIR" rev-parse HEAD)"
	echo "ghostty_gen_zig     $GHOSTTY_ZIG"
	echo "doom_repo           $DOOM_REPO"
	echo "doom_commit         $(git -C "$DOOM_DIR" rev-parse HEAD)"
	echo "doom_zig            $DOOM_ZIG"
	echo "doom_status         $DOOM_STATUS (intro demo patched out)"
	echo "fixture_bytes       $FIXTURE_BYTES"
	echo "utf8_seed           $UTF8_SEED"
	echo "ascii_seed          <time-based; upstream +ascii has no --seed>"
	for f in "$FIXTURE_DIR"/150MB_*.txt; do
		[ -f "$f" ] || continue
		echo "sha256              $(sha256_of "$f")  $(basename "$f")"
	done
} >"$FIXTURE_DIR/PROVENANCE.txt"

log "fixtures ready:"
sed 's/^/    /' "$FIXTURE_DIR/PROVENANCE.txt" >&2
