#!/usr/bin/env bats

setup() {
	bats_load_library xberg-bats
	xberg_setup
	SYNC="$(cd "$BATS_TEST_DIRNAME/../../.." && pwd -P)/scripts/task/sync-bats-lib.sh"
	export BATS_LIB_DEST="$XBERG_WORK/vendored"
	export BATS_LIB_REF=793d3a4d37a17009d91cce29f2825eda570d2abd
	mkdir -p "$BATS_LIB_DEST"
	printf 'published helper\n' >"$XBERG_WORK/source.bash"
	export HELPER_SOURCE="$XBERG_WORK/source.bash"
	xberg_stub curl \
		'printf "%s\n" "$@" >>"$XBERG_TRACE"' \
		'while [ "$#" -gt 0 ]; do' \
		'  if [ "$1" = --output ]; then target="$2"; shift; fi' \
		'  shift' \
		'done' \
		'cp "$HELPER_SOURCE" "$target"'
}

@test "sync should reject a missing ref without fetching" {
	run env BATS_LIB_REF= bash "$SYNC" sync
	xberg_assert_status 1
	xberg_assert_output_contains 'must name an immutable'
	xberg_assert_trace_empty
}

@test "sync should reject a branch without fetching" {
	run env BATS_LIB_REF=main bash "$SYNC" sync
	xberg_assert_status 1
	xberg_assert_output_contains 'not '\''main'\'''
	xberg_assert_trace_empty
}

@test "sync should reject the floating major tag without fetching" {
	run env BATS_LIB_REF=v1 bash "$SYNC" sync
	xberg_assert_status 1
	xberg_assert_trace_empty
}

@test "sync should reject an unknown mode before fetching" {
	run bash "$SYNC" typo
	xberg_assert_status 2
	xberg_assert_output_contains 'usage:'
	xberg_assert_trace_empty
}

@test "sync should copy the exact helper from the pinned repository revision" {
	run bash "$SYNC" sync
	xberg_assert_status 0
	cmp "$HELPER_SOURCE" "$BATS_LIB_DEST/load.bash"
	[ "$(tail -n 1 "$XBERG_TRACE")" = "https://raw.githubusercontent.com/xberg-io/actions/$BATS_LIB_REF/bats-lib/xberg-bats/load.bash" ]
}

@test "check should accept byte identical vendored contents" {
	cp "$HELPER_SOURCE" "$BATS_LIB_DEST/load.bash"
	run bash "$SYNC" --check
	xberg_assert_status 0
	xberg_assert_output_contains 'matches xberg-io/actions@'
}

@test "check should reject changed helper bytes without replacing them" {
	printf 'local drift\n' >"$BATS_LIB_DEST/load.bash"
	run bash "$SYNC" --check
	xberg_assert_status 1
	xberg_assert_output_contains 'has drifted'
	xberg_assert_file "$BATS_LIB_DEST/load.bash" 'local drift'
}

@test "check should reject a missing vendored helper" {
	run bash "$SYNC" --check
	xberg_assert_status 1
	xberg_assert_output_contains 'has drifted'
}

@test "sync should preserve the vendored helper when the source fetch fails" {
	printf 'existing helper\n' >"$BATS_LIB_DEST/load.bash"
	xberg_stub_exit curl 22
	run bash "$SYNC" sync
	xberg_assert_status 22
	xberg_assert_file "$BATS_LIB_DEST/load.bash" 'existing helper'
}
