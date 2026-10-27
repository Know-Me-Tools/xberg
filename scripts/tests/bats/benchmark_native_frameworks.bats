#!/usr/bin/env bats

setup() {
	bats_load_library xberg-bats
	xberg_setup
	REPOSITORY_ROOT="$(cd "$BATS_TEST_DIRNAME/../../.." && pwd -P)"
	source "$REPOSITORY_ROOT/tools/benchmark-harness/scripts/bench_local_frameworks.sh"
}

@test "native batch selection should retain supported frameworks in first occurrence order" {
	run native_batch_frameworks 'markitdown,xberg-markdown-layout,docling,unstructured,liteparse,xberg-markdown-baseline,xberg-markdown-layout'
	xberg_assert_status 0
	xberg_assert_output 'xberg-markdown-layout,docling,liteparse,xberg-markdown-baseline'
}

@test "native batch selection should accept an empty default selection" {
	run native_batch_frameworks ''
	xberg_assert_status 0
	xberg_assert_no_output
}

@test "native batch validation should deduplicate explicitly supported frameworks" {
	run validate_native_batch_frameworks 'docling,xberg-markdown-paddle-ocr-batch,liteparse,docling'
	xberg_assert_status 0
	xberg_assert_output 'docling,xberg-markdown-paddle-ocr-batch,liteparse'
}

@test "native batch validation should reject an unsupported explicit framework" {
	run validate_native_batch_frameworks 'xberg-private,unstructured'
	xberg_assert_status 1
	xberg_assert_output_contains 'unsupported native-batch framework: xberg-private'
}

@test "native batch validation should reject empty entries instead of measuring a partial list" {
	local malformed
	for malformed in ',docling' 'docling,' 'docling,,liteparse' ','; do
		run validate_native_batch_frameworks "$malformed"
		xberg_assert_status 1
		xberg_assert_output_contains "malformed native-batch framework list: $malformed"
	done
}

@test "framework membership should match a whole comma separated entry" {
	run framework_list_contains 'xberg-markdown-baseline,docling' docling
	xberg_assert_status 0
	run framework_list_contains 'xberg-markdown-baseline,liteparse' docling
	xberg_assert_status 1
	run framework_list_contains 'xberg-markdown-baseline,docling-other' docling
	xberg_assert_status 1
}

@test "Docling preflight should include an explicitly selected batch only framework" {
	run docling_is_explicitly_requested 'xberg-markdown-baseline' 'docling,liteparse'
	xberg_assert_status 0
	run docling_is_explicitly_requested 'xberg-markdown-baseline' 'liteparse'
	xberg_assert_status 1
}
