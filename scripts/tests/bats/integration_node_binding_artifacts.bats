#!/usr/bin/env bats

setup() {
	bats_load_library xberg-bats
	xberg_setup
	REPOSITORY_ROOT="$(cd "$BATS_TEST_DIRNAME/../../.." && pwd -P)"
	cd "$XBERG_WORK" || return
	cp "$REPOSITORY_ROOT/.task/tools/integrations.yml" integrations.yml
	mkdir -p crates/xberg-node
	cp "$REPOSITORY_ROOT/crates/xberg-node/package.json" crates/xberg-node/
	printf 'reviewed loader\n' >crates/xberg-node/index.js
	printf 'export wrapper\n' >crates/xberg-node/index.mjs
	printf 'binding wrapper\n' >crates/xberg-node/binding.cjs
	printf 'type declarations\n' >crates/xberg-node/index.d.ts
	cat >Taskfile.yml <<'TASK'
version: '3'
includes:
  integrations: ./integrations.yml
tasks:
  build:
    cmds:
      - task: integrations:ci:build:node-binding
TASK
	unset INSTALL_STATUS BUILD_STATUS
	xberg_stub pnpm \
		'printf "%s|jobs=%s\n" "$*" "${CARGO_BUILD_JOBS:-unset}" >>"$XBERG_TRACE"' \
		'if [ "$1" = install ]; then exit "${INSTALL_STATUS:-0}"; fi' \
		'case " $* " in *" --no-js "*) ;; *) printf "unreviewed generated loader\n" >index.js ;; esac' \
		'printf "native artifact\n" >fixture.node' \
		'exit "${BUILD_STATUS:-0}"'
}

@test "integration Node binding build should stage every exported entry point and preserve the reviewed loader" {
	run task build
	xberg_assert_status 0
	xberg_assert_trace 'install|jobs=unset
exec napi build --no-js --output-dir . --release --platform|jobs=1'
	xberg_assert_file crates/xberg-node/index.js 'reviewed loader'
	xberg_assert_file dist/node/index.js 'reviewed loader'
	xberg_assert_file dist/node/index.mjs 'export wrapper'
	xberg_assert_file dist/node/binding.cjs 'binding wrapper'
	xberg_assert_file dist/node/index.d.ts 'type declarations'
	xberg_assert_file dist/node/fixture.node 'native artifact'
	cmp crates/xberg-node/package.json dist/node/package.json
	[ "$(find dist/node -type f | wc -l | tr -d '[:space:]')" -eq 6 ]
}

@test "integration Node binding build should stop before compilation when installation fails" {
	export INSTALL_STATUS=7
	run task build
	[ "$status" -ne 0 ]
	xberg_assert_trace 'install|jobs=unset'
	[ ! -e dist/node ]
}

@test "integration Node binding build should reject a failed compiler despite an emitted native artifact" {
	export BUILD_STATUS=9
	run task build
	[ "$status" -ne 0 ]
	[ -f crates/xberg-node/fixture.node ]
	[ ! -e dist/node ]
}
