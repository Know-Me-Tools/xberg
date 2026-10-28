#!/usr/bin/env bats

setup() {
	bats_load_library xberg-bats
	xberg_setup
	REPOSITORY_ROOT="$(cd "$BATS_TEST_DIRNAME/../../.." && pwd -P)"
	SCRIPT="$REPOSITORY_ROOT/scripts/publish/pack-node-platform-packages.sh"
	unset NPM_PACK_STATUS
	xberg_stub npm \
		'printf "npm %s in %s\n" "$*" "${PWD##*/}" >>"$XBERG_TRACE"' \
		'[ "$*" = pack ] || exit 97' \
		'[ "${NPM_PACK_STATUS:-0}" -eq 0 ] || exit "$NPM_PACK_STATUS"' \
		'tar -czf "fixture-${PWD##*/}-1.4.0.tgz" package.json ./*.node'
	cd "$XBERG_WORK"
}

write_platform() {
	local platform="$1"
	mkdir -p "npm/$platform"
	printf '{"name":"@xberg-io/%s","version":"1.4.0"}\n' "$platform" >"npm/$platform/package.json"
	printf 'native fixture for %s\n' "$platform" >"npm/$platform/xberg.node"
}

@test "Node platform pack should reject a missing package root without invoking npm" {
	run bash "$SCRIPT" "$XBERG_WORK/absent"
	xberg_assert_status 1
	xberg_assert_output_contains 'npm directory not found'
	xberg_assert_trace_empty
}

@test "Node platform pack should reject a manifest without a native binary before npm pack" {
	mkdir -p npm/linux-x64
	printf '{"name":"@xberg-io/linux-x64","version":"1.4.0"}\n' >npm/linux-x64/package.json
	run bash "$SCRIPT" "$XBERG_WORK/npm"
	xberg_assert_status 1
	xberg_assert_output_contains 'Platform package missing .node binary: linux-x64/'
	xberg_assert_trace_empty
	[ "$(find npm -name '*.tgz' | wc -l | tr -d '[:space:]')" -eq 0 ]
}

@test "Node platform pack should move real archives to the root and skip folders without manifests" {
	write_platform linux-x64
	write_platform darwin-arm64
	mkdir -p npm/not-a-package
	printf 'not a package\n' >npm/not-a-package/README.md
	run bash "$SCRIPT" "$XBERG_WORK/npm"
	xberg_assert_status 0
	xberg_assert_output_contains 'Successfully packed 2 platform package(s)'
	xberg_assert_output_contains '  File size: 29'
	xberg_assert_output_contains '  File size: 32'
	[ "$(printf '%s\n' "$output" | grep -c '^  File size: [0-9][0-9]*$')" -eq 2 ]
	[[ "$output" != *'stat:'* ]]
	[[ "$output" != *'linux-x64/linux-x64/'* ]]
	[[ "$output" != *'darwin-arm64/darwin-arm64/'* ]]
	xberg_assert_trace 'npm pack in darwin-arm64' 'npm pack in linux-x64'
	[ "$(find npm -maxdepth 1 -name '*.tgz' | wc -l | tr -d '[:space:]')" -eq 2 ]
	[ "$(find npm -mindepth 2 -name '*.tgz' | wc -l | tr -d '[:space:]')" -eq 0 ]
	tar -tzf npm/fixture-linux-x64-1.4.0.tgz >"$XBERG_WORK/archive-entries"
	grep -Fxq package.json "$XBERG_WORK/archive-entries"
	grep -Fxq ./xberg.node "$XBERG_WORK/archive-entries"
	[ "$(wc -l <"$XBERG_WORK/archive-entries" | tr -d '[:space:]')" -eq 2 ]
	[ "$(tar -xOzf npm/fixture-linux-x64-1.4.0.tgz ./xberg.node)" = 'native fixture for linux-x64' ]
}

@test "Node platform pack should propagate npm failure without reporting success or moving an archive" {
	write_platform linux-x64
	export NPM_PACK_STATUS=42
	run bash "$SCRIPT" "$XBERG_WORK/npm"
	xberg_assert_status 1
	xberg_assert_output_contains 'Failed to pack linux-x64/'
	[[ "$output" != *'Successfully packed'* ]]
	xberg_assert_trace 'npm pack in linux-x64'
	[ "$(find npm -name '*.tgz' | wc -l | tr -d '[:space:]')" -eq 0 ]
}

@test "Node platform pack should stop before npm when binary size inspection fails" {
	write_platform linux-x64
	xberg_stub stat 'printf "%s\n" "stat unavailable fixture" >&2' 'exit 42'
	run bash "$SCRIPT" "$XBERG_WORK/npm"
	xberg_assert_status 42
	xberg_assert_output_contains 'stat unavailable fixture'
	[[ "$output" != *'Running npm pack'* ]]
	xberg_assert_trace_empty
	[ "$(find npm -name '*.tgz' | wc -l | tr -d '[:space:]')" -eq 0 ]
}
