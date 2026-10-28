#!/usr/bin/env bats

setup() {
	bats_load_library xberg-bats
	xberg_setup
	REPOSITORY_ROOT="$(cd "$BATS_TEST_DIRNAME/../../.." && pwd -P)"
	unset RID RUST_TARGET FFI_NAME RUNNER_OS
	cd "$XBERG_WORK"
}

write_nuget() {
	mkdir -p packages/csharp/artifacts/csharp
	python3 - "$@" <<'PY'
import sys
import zipfile

with zipfile.ZipFile("packages/csharp/artifacts/csharp/Xberg.1.4.0.nupkg", "w") as package:
    for rid in sys.argv[1:]:
        suffix = "dll" if rid == "win-x64" else "dylib" if rid == "osx-arm64" else "so"
        package.writestr(f"runtimes/{rid}/native/xberg_ffi.{suffix}", f"fixture for {rid}\n")
PY
}

@test "native directory guard should reject an absent RID before examining assets" {
	run bash "$REPOSITORY_ROOT/scripts/publish/csharp/verify-native-assets-dir.sh"
	xberg_assert_status 2
	xberg_assert_output_contains '(or set RID)'
}

@test "native directory guard should reject missing and empty asset directories" {
	run bash "$REPOSITORY_ROOT/scripts/publish/csharp/verify-native-assets-dir.sh" linux-x64
	xberg_assert_status 1
	xberg_assert_output_contains 'Native assets directory not found: runtimes/linux-x64/native'
	mkdir -p runtimes/linux-x64/native
	run env RID=linux-x64 bash "$REPOSITORY_ROOT/scripts/publish/csharp/verify-native-assets-dir.sh"
	xberg_assert_status 1
	xberg_assert_output_contains 'No native asset files found'
}

@test "native directory guard should count actual files only under the selected RID" {
	mkdir -p runtimes/linux-x64/native/subdir runtimes/win-x64/native
	printf 'ffi\n' >runtimes/linux-x64/native/xberg_ffi.so
	printf 'dependency\n' >runtimes/linux-x64/native/subdir/dependency.so
	printf 'other platform\n' >runtimes/win-x64/native/xberg_ffi.dll
	run env RID=linux-x64 bash "$REPOSITORY_ROOT/scripts/publish/csharp/verify-native-assets-dir.sh" win-x64
	xberg_assert_status 0
	[ "$output" = 'Found 2 native asset files' ]
}

@test "native assembly should prefer target-specific FFI and copy only platform dependencies" {
	mkdir -p target/aarch64-unknown-linux-gnu/release target/release target/csharp-native/linux-arm64/subdir
	printf 'target-specific\n' >target/aarch64-unknown-linux-gnu/release/libxberg_ffi.so
	printf 'fallback\n' >target/release/libxberg_ffi.so
	printf 'runtime\n' >target/csharp-native/linux-arm64/libonnxruntime.so.1
	printf 'wrong platform\n' >target/csharp-native/linux-arm64/runtime.dll
	printf 'nested\n' >target/csharp-native/linux-arm64/subdir/nested.so
	run env RID=linux-arm64 RUST_TARGET=aarch64-unknown-linux-gnu FFI_NAME=libxberg_ffi.so RUNNER_OS=Linux \
		bash "$REPOSITORY_ROOT/scripts/publish/csharp/assemble-native-assets.sh"
	xberg_assert_status 0
	[ "$(cat runtimes/linux-arm64/native/libxberg_ffi.so)" = target-specific ]
	[ "$(cat runtimes/linux-arm64/native/libonnxruntime.so.1)" = runtime ]
	[ "$(find runtimes/linux-arm64/native -type f | wc -l | tr -d '[:space:]')" -eq 2 ]
}

@test "native assembly should use the fallback FFI with only Windows DLL dependencies" {
	mkdir -p target/release target/csharp-native/win-x64
	printf 'fallback dll\n' >target/release/xberg_ffi.dll
	printf 'runtime dll\n' >target/csharp-native/win-x64/onnxruntime.dll
	printf 'wrong platform\n' >target/csharp-native/win-x64/libonnxruntime.so
	run env RID=win-x64 RUST_TARGET=x86_64-pc-windows-msvc FFI_NAME=xberg_ffi.dll RUNNER_OS=Windows \
		bash "$REPOSITORY_ROOT/scripts/publish/csharp/assemble-native-assets.sh"
	xberg_assert_status 0
	[ "$(cat runtimes/win-x64/native/xberg_ffi.dll)" = 'fallback dll' ]
	[ "$(cat runtimes/win-x64/native/onnxruntime.dll)" = 'runtime dll' ]
	[ "$(find runtimes/win-x64/native -type f | wc -l | tr -d '[:space:]')" -eq 2 ]
}

@test "native assembly should fail before copying dependencies when both FFI paths are absent" {
	mkdir -p target/csharp-native/linux-x64
	printf 'dependency\n' >target/csharp-native/linux-x64/libonnxruntime.so
	run env RID=linux-x64 RUST_TARGET=x86_64-unknown-linux-gnu FFI_NAME=libxberg_ffi.so RUNNER_OS=Linux \
		bash "$REPOSITORY_ROOT/scripts/publish/csharp/assemble-native-assets.sh"
	xberg_assert_status 1
	xberg_assert_output_contains 'FFI library missing at both'
	[ "$(find runtimes/linux-x64/native -type f | wc -l | tr -d '[:space:]')" -eq 0 ]
}

@test "NuGet guard should examine a genuine archive containing all four required RID assets" {
	write_nuget linux-arm64 linux-x64 osx-arm64 win-x64
	run bash "$REPOSITORY_ROOT/scripts/publish/csharp/verify-nuget-native-assets.sh"
	xberg_assert_status 0
	xberg_assert_output_contains 'All native assets verified successfully'
	[ "$(printf '%s\n' "$output" | grep -c 'Found xberg_ffi for')" -eq 4 ]
}

@test "NuGet guard should reject a real archive missing two required platforms" {
	write_nuget linux-x64 osx-arm64
	run bash "$REPOSITORY_ROOT/scripts/publish/csharp/verify-nuget-native-assets.sh"
	xberg_assert_status 1
	xberg_assert_output_contains 'Missing xberg_ffi binary for linux-arm64'
	xberg_assert_output_contains 'Missing xberg_ffi binary for win-x64'
	xberg_assert_output_contains '::error::Missing 2 native asset(s) in NuGet package'
}

@test "NuGet guard should reject an empty artifact directory instead of an empty pass" {
	mkdir -p packages/csharp/artifacts/csharp
	run bash "$REPOSITORY_ROOT/scripts/publish/csharp/verify-nuget-native-assets.sh"
	xberg_assert_status 1
	xberg_assert_output_contains 'No .nupkg found under packages/csharp/artifacts/csharp'
}
