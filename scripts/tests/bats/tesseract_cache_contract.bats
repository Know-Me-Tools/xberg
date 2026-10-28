#!/usr/bin/env bats

setup() {
	bats_load_library xberg-bats
	xberg_setup
	xberg_setup_workspace
	REPOSITORY_ROOT="$(cd "$BATS_TEST_DIRNAME/../../.." && pwd -P)"
	CACHE_SCRIPTS="$REPOSITORY_ROOT/scripts/ci/actions/setup-tesseract-cache"
	unset TEST_MULTIARCH TEST_ARCH
	xberg_stub dpkg-architecture 'printf "%s\n" "${TEST_MULTIARCH:-}"'
	xberg_stub uname 'printf "%s\n" "${TEST_ARCH:-x86_64}"'
}

@test "Tesseract cache outputs should use the detected Debian multiarch library directory" {
	export TEST_MULTIARCH=aarch64-linux-gnu
	run bash "$CACHE_SCRIPTS/set-outputs.sh" linux-arm64 true
	xberg_assert_status 0
	xberg_assert_github_env \
		"TESSERACT_RS_CACHE_DIR=$GITHUB_WORKSPACE/.tesseract-cache/linux-arm64
XDG_CACHE_HOME=$GITHUB_WORKSPACE/.xdg-cache/linux-arm64"
	xberg_assert_github_output \
		"cache-dir=$GITHUB_WORKSPACE/.tesseract-cache/linux-arm64
cache-enabled=true
docker-options=--env TESSERACT_RS_CACHE_DIR=/io/.tesseract-cache/linux-arm64 --env XDG_CACHE_HOME=/io/.xdg-cache/linux-arm64 --env OPENSSL_LIB_DIR=/usr/lib/aarch64-linux-gnu --env OPENSSL_INCLUDE_DIR=/usr/include"
}

@test "Tesseract cache outputs should fall back to x86 multiarch when Debian detection is empty" {
	export TEST_ARCH=x86_64
	run bash "$CACHE_SCRIPTS/set-outputs.sh" linux-x64 true
	xberg_assert_status 0
	grep -Fq -- '--env OPENSSL_LIB_DIR=/usr/lib/x86_64-linux-gnu --env OPENSSL_INCLUDE_DIR=/usr/include' "$GITHUB_OUTPUT"
}

@test "Tesseract cache outputs should normalize arm64 uname to Debian multiarch" {
	export TEST_ARCH=arm64
	run bash "$CACHE_SCRIPTS/set-outputs.sh" linux-arm64 true
	xberg_assert_status 0
	grep -Fq -- '--env OPENSSL_LIB_DIR=/usr/lib/aarch64-linux-gnu --env OPENSSL_INCLUDE_DIR=/usr/include' "$GITHUB_OUTPUT"
}

@test "Tesseract cache outputs should use the generic library directory for unknown architectures" {
	export TEST_ARCH=riscv64
	run bash "$CACHE_SCRIPTS/set-outputs.sh" linux-riscv64 true
	xberg_assert_status 0
	grep -Fq -- '--env OPENSSL_LIB_DIR=/usr/lib --env OPENSSL_INCLUDE_DIR=/usr/include' "$GITHUB_OUTPUT"
}

@test "disabled Tesseract cache should clear its outputs without configuring Docker" {
	run bash "$CACHE_SCRIPTS/set-outputs.sh" disabled false
	xberg_assert_status 0
	xberg_assert_github_env 'TESSERACT_RS_CACHE_DIR='
	xberg_assert_github_output 'cache-dir=
cache-enabled=false
docker-options='
}

@test "Tesseract cache outputs should reject missing enablement before writing output" {
	run bash "$CACHE_SCRIPTS/set-outputs.sh" linux-x64
	[ "$status" -ne 0 ]
	xberg_assert_output_contains 'enable-cache required (true/false)'
	xberg_assert_github_output ''
	xberg_assert_github_env ''
}

@test "Tesseract cache setup should isolate both cache directories by label and preserve existing files" {
	cd "$XBERG_WORK"
	run bash "$CACHE_SCRIPTS/setup-dirs.sh" linux-x64
	xberg_assert_status 0
	printf 'cached bytes\n' >.tesseract-cache/linux-x64/artifact
	run bash "$CACHE_SCRIPTS/setup-dirs.sh" linux-x64
	xberg_assert_status 0
	[ -d .xdg-cache/linux-x64 ]
	xberg_assert_file .tesseract-cache/linux-x64/artifact 'cached bytes'
	[ "$(find .tesseract-cache .xdg-cache -type d | wc -l | tr -d '[:space:]')" -eq 4 ]
}

@test "Tesseract cache setup should reject a missing label without creating directories" {
	cd "$XBERG_WORK"
	run bash "$CACHE_SCRIPTS/setup-dirs.sh"
	[ "$status" -ne 0 ]
	xberg_assert_output_contains 'label required'
	[ ! -e .tesseract-cache ]
	[ ! -e .xdg-cache ]
}
