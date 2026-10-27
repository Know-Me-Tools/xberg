#!/usr/bin/env bats

setup() {
	bats_load_library xberg-bats
	xberg_setup
	REPOSITORY_ROOT="$(cd "$BATS_TEST_DIRNAME/../../.." && pwd -P)"
	unset XBERG_MAX_THREADS COHORT SHARD PDF_BACKENDS XBERG_BENCH_PROFILE
	unset HEURISTIC_COHORT OCR_COHORT BATCH_HEURISTIC_COHORT BATCH_OCR_COHORT
	unset OCR_FIXTURES BATCH_OCR_FIXTURES BATCH_SIZE BATCH_WORKERS
	export FIXTURES="$XBERG_WORK/fixtures"
	mkdir -p "$FIXTURES"
	printf '%s\n' '{"metadata":{"requires_ocr":false}}' >"$FIXTURES/heuristic.json"
	xberg_stub benchmark-harness 'printf "%s\n" "$*" >>"$XBERG_TRACE"'
	xberg_stub_exit xberg 0
	# ~keep The local wrapper must stay on its SKIP_BUILD path; this suite never invokes Cargo.
	xberg_stub cargo 'echo "unexpected build command" >&2' 'exit 97'
}

run_ci_benchmark() {
	local mode="$1" budget="${2:-}"
	local args=(FRAMEWORK=xberg-markdown-baseline MODE="$mode" FIXTURES_DIR="$FIXTURES" HARNESS_PATH="$XBERG_STUB_BIN/benchmark-harness")
	[ -z "$budget" ] || args+=(XBERG_MAX_THREADS="$budget")
	(cd "$XBERG_WORK" && env "${args[@]}" bash "$REPOSITORY_ROOT/scripts/benchmarks/run-benchmark.sh")
}

run_local_benchmark() {
	local budget="${1:-}"
	local args=(HEURISTIC_FIXTURES="$FIXTURES" BATCH_HEURISTIC_FIXTURES="$FIXTURES" FRAMEWORKS=xberg-markdown-baseline BATCH_FRAMEWORKS=xberg-markdown-baseline-batch HARNESS="$XBERG_STUB_BIN/benchmark-harness" XBERG_CLI_BINARY="$XBERG_STUB_BIN/xberg" OUT="$XBERG_WORK/results" SKIP_BUILD=1)
	[ -z "$budget" ] || args+=(XBERG_MAX_THREADS="$budget")
	env "${args[@]}" bash "$REPOSITORY_ROOT/tools/benchmark-harness/scripts/bench_local.sh"
}

assert_harness_budget() {
	local mode="$1" concurrent="$2" budget="$3"
	grep -F -- "--mode $mode --max-concurrent $concurrent --xberg-max-threads $budget" "$XBERG_TRACE"
}

@test "CI benchmark should use the same default CPU budget in single file and batch modes" {
	run run_ci_benchmark single-file
	xberg_assert_status 0
	run run_ci_benchmark batch
	xberg_assert_status 0
	[ "$(wc -l <"$XBERG_TRACE" | tr -d '[:space:]')" -eq 2 ]
	assert_harness_budget single-file 1 4
	assert_harness_budget batch 4 4
}

@test "CI benchmark should forward the explicit CPU budget without changing concurrency" {
	run run_ci_benchmark single-file 7
	xberg_assert_status 0
	[ "$(wc -l <"$XBERG_TRACE" | tr -d '[:space:]')" -eq 1 ]
	assert_harness_budget single-file 1 7
}

@test "local benchmark should use the same default CPU budget for both measured modes" {
	run run_local_benchmark
	xberg_assert_status 0
	[ "$(wc -l <"$XBERG_TRACE" | tr -d '[:space:]')" -eq 3 ]
	[ "$(grep -c '^run ' "$XBERG_TRACE")" -eq 2 ]
	[ "$(grep -c '^gap-report ' "$XBERG_TRACE")" -eq 1 ]
	assert_harness_budget single-file 1 4
	assert_harness_budget batch 4 4
}

@test "local benchmark should forward one explicit CPU budget to both measured modes" {
	run run_local_benchmark 7
	xberg_assert_status 0
	[ "$(wc -l <"$XBERG_TRACE" | tr -d '[:space:]')" -eq 3 ]
	[ "$(grep -c '^run ' "$XBERG_TRACE")" -eq 2 ]
	[ "$(grep -c '^gap-report ' "$XBERG_TRACE")" -eq 1 ]
	assert_harness_budget single-file 1 7
	assert_harness_budget batch 4 7
}
