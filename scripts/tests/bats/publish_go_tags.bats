#!/usr/bin/env bats

setup() {
	bats_load_library xberg-bats
	xberg_setup
	REPOSITORY_ROOT="$(cd "$BATS_TEST_DIRNAME/../../.." && pwd -P)"
	SCRIPT="$REPOSITORY_ROOT/scripts/publish/go/tag-and-push-go-module.sh"
	unset LOCAL_TAG_EXISTS REMOTE_TAG_EXISTS GIT_PUSH_STATUS GH_API_STATUS
	export GITHUB_REPOSITORY=example/project
	xberg_stub git \
		'printf "git %s\n" "$*" >>"$XBERG_TRACE"' \
		'case "$1" in' \
		'  rev-parse)' \
		'    if [ "$2" = "v1.4.0^{commit}" ]; then printf "%s\n" 0123456789abcdef0123456789abcdef01234567; exit 0; fi' \
		'    exit "${LOCAL_TAG_EXISTS:-1}" ;;' \
		'  ls-remote)' \
		'    if [ "${REMOTE_TAG_EXISTS:-0}" = 1 ]; then' \
		'      printf "deadbeef\trefs/tags/packages/go/v1/v1.4.0\ndeadbeef\trefs/tags/packages/go/v1.4.0\n"' \
		'    fi; exit 0 ;;' \
		'  tag) exit 0 ;;' \
		'  push) exit "${GIT_PUSH_STATUS:-0}" ;;' \
		'  *) echo "unexpected git invocation" >&2; exit 97 ;;' \
		'esac'
	xberg_stub gh \
		'printf "gh %s\n" "$*" >>"$XBERG_TRACE"' \
		'exit "${GH_API_STATUS:-0}"'
}

@test "Go publication should create and push both annotated module tags at the release ref" {
	run bash "$SCRIPT" v1.4.0
	xberg_assert_status 0
	xberg_assert_trace \
		'git rev-parse v1.4.0^{commit}' \
		'git rev-parse packages/go/v1/v1.4.0' \
		'git ls-remote --tags origin' \
		'git tag -a packages/go/v1/v1.4.0 v1.4.0 -m Go module tag packages/go/v1/v1.4.0' \
		'git push origin refs/tags/packages/go/v1/v1.4.0' \
		'git rev-parse packages/go/v1.4.0' \
		'git ls-remote --tags origin' \
		'git tag -a packages/go/v1.4.0 v1.4.0 -m Go module tag packages/go/v1.4.0' \
		'git push origin refs/tags/packages/go/v1.4.0'
}

@test "Go publication should leave existing local module tags untouched" {
	export LOCAL_TAG_EXISTS=0
	run bash "$SCRIPT" v1.4.0
	xberg_assert_status 0
	xberg_assert_trace \
		'git rev-parse v1.4.0^{commit}' \
		'git rev-parse packages/go/v1/v1.4.0' \
		'git rev-parse packages/go/v1.4.0'
	[ "$(printf '%s\n' "$output" | grep -c 'already exists locally; skipping')" -eq 2 ]
}

@test "Go publication should leave existing remote module tags untouched" {
	export REMOTE_TAG_EXISTS=1
	run bash "$SCRIPT" v1.4.0
	xberg_assert_status 0
	xberg_assert_trace \
		'git rev-parse v1.4.0^{commit}' \
		'git rev-parse packages/go/v1/v1.4.0' \
		'git ls-remote --tags origin' \
		'git rev-parse packages/go/v1.4.0' \
		'git ls-remote --tags origin'
	[ "$(printf '%s\n' "$output" | grep -c 'already exists on remote; skipping')" -eq 2 ]
}

@test "Go publication should fall back to the API with the exact resolved commit when push fails" {
	export GIT_PUSH_STATUS=42
	run bash "$SCRIPT" v1.4.0
	xberg_assert_status 0
	grep -Fxq 'gh api repos/example/project/git/refs -f ref=refs/tags/packages/go/v1/v1.4.0 -f sha=0123456789abcdef0123456789abcdef01234567 --silent' "$XBERG_TRACE"
	grep -Fxq 'gh api repos/example/project/git/refs -f ref=refs/tags/packages/go/v1.4.0 -f sha=0123456789abcdef0123456789abcdef01234567 --silent' "$XBERG_TRACE"
	[ "$(grep -c '^gh api ' "$XBERG_TRACE")" -eq 2 ]
}

@test "Go publication should stop before the legacy tag if its API fallback fails" {
	export GIT_PUSH_STATUS=1 GH_API_STATUS=42
	run bash "$SCRIPT" v1.4.0
	xberg_assert_status 42
	grep -Fxq 'gh api repos/example/project/git/refs -f ref=refs/tags/packages/go/v1/v1.4.0 -f sha=0123456789abcdef0123456789abcdef01234567 --silent' "$XBERG_TRACE"
	[ "$(grep -c '^gh api ' "$XBERG_TRACE")" -eq 1 ]
	! grep -Fq 'git rev-parse packages/go/v1.4.0' "$XBERG_TRACE"
}

@test "Go publication should reject a missing release tag before invoking git or GitHub" {
	run bash "$SCRIPT"
	[ "$status" -ne 0 ]
	xberg_assert_output_contains 'Release tag argument required'
	xberg_assert_trace_empty
}
