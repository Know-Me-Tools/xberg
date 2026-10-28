#!/usr/bin/env bats

setup() {
	bats_load_library xberg-bats
	xberg_setup
	REPOSITORY_ROOT="$(cd "$BATS_TEST_DIRNAME/../../.." && pwd -P)"
	unset DOCKER_TAG SUMMARY_LABEL IMAGE VERSION TAG_SUFFIX
	xberg_stub docker \
		'printf "%s\n" "$*" >>"$XBERG_TRACE"' \
		'exit "${DOCKER_MANIFEST_STATUS:-0}"'
	unset DOCKER_MANIFEST_STATUS
}

@test "Docker tag guard should record an existing tag and its skipped publish summary" {
	run env DOCKER_TAG=example.invalid/xberg:1.4.0 SUMMARY_LABEL=core \
		bash "$REPOSITORY_ROOT/scripts/publish/check-docker-tag.sh"
	xberg_assert_status 0
	xberg_assert_trace 'manifest inspect example.invalid/xberg:1.4.0'
	xberg_assert_github_output 'exists=true'
	xberg_assert_github_summary 'Docker tag example.invalid/xberg:1.4.0 already exists; core publish will be skipped.'
}

@test "Docker tag guard should record a missing tag without a skip summary" {
	export DOCKER_MANIFEST_STATUS=1
	run env DOCKER_TAG=example.invalid/xberg:1.4.0 \
		bash "$REPOSITORY_ROOT/scripts/publish/check-docker-tag.sh"
	xberg_assert_status 0
	xberg_assert_trace 'manifest inspect example.invalid/xberg:1.4.0'
	xberg_assert_github_output 'exists=false'
	xberg_assert_github_summary ''
}

@test "Docker tag guard should reject a missing tag before calling Docker" {
	run bash "$REPOSITORY_ROOT/scripts/publish/check-docker-tag.sh"
	[ "$status" -ne 0 ]
	xberg_assert_output_contains 'DOCKER_TAG not set'
	xberg_assert_trace_empty
	xberg_assert_github_output ''
}

@test "Docker dry run should report the exact image version and suffix without calling Docker" {
	run env IMAGE=example.invalid/xberg VERSION=1.4.0 TAG_SUFFIX=-core \
		bash "$REPOSITORY_ROOT/scripts/publish/docker/dry-run-summary.sh"
	xberg_assert_status 0
	xberg_assert_github_summary \
		'Dry run requested; Docker image example.invalid/xberg:1.4.0-core tested but not pushed.'
	xberg_assert_trace_empty
}

@test "Docker dry run should reject incomplete metadata without writing a summary" {
	run env IMAGE=example.invalid/xberg bash "$REPOSITORY_ROOT/scripts/publish/docker/dry-run-summary.sh"
	xberg_assert_status 2
	xberg_assert_output_contains 'Usage: set IMAGE and VERSION'
	xberg_assert_github_summary ''
	xberg_assert_trace_empty
}

@test "Go artifact staging should create only the expected private directories and be idempotent" {
	cd "$XBERG_WORK"
	run bash "$REPOSITORY_ROOT/scripts/publish/go/create-artifact-staging-dir.sh"
	xberg_assert_status 0
	run bash "$REPOSITORY_ROOT/scripts/publish/go/create-artifact-staging-dir.sh"
	xberg_assert_status 0
	[ -d artifact-staging/xberg-ffi/lib ]
	[ -d artifact-staging/xberg-ffi/include ]
	[ -d artifact-staging/xberg-ffi/share/pkgconfig ]
	[ "$(find artifact-staging -type d | wc -l | tr -d '[:space:]')" -eq 6 ]
	[ "$(find artifact-staging -type f | wc -l | tr -d '[:space:]')" -eq 0 ]
}
