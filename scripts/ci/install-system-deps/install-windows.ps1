Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

Write-Host "::group::Installing Windows dependencies"

function Retry-Command {
  param(
    [scriptblock]$Command,
    [int]$MaxAttempts = 3,
    [int]$DelaySeconds = 5
  )

  $attempt = 1
  while ($attempt -le $MaxAttempts) {
    try {
      Write-Host "Attempt $attempt of $MaxAttempts..."
      $global:LASTEXITCODE = 0
      & $Command | Out-Host
      if ($LASTEXITCODE -ne 0) { throw "Native dependency command failed with exit $LASTEXITCODE" }
      return $true
    }
    catch {
      $attempt++
      if ($attempt -le $MaxAttempts) {
        $backoffDelay = $DelaySeconds * [Math]::Pow(2, $attempt - 1)
        Write-Host "⚠ Attempt failed, retrying in ${backoffDelay}s..." -ForegroundColor Yellow
        Start-Sleep -Seconds $backoffDelay
      }
      else {
        return $false
      }
    }
  }
}

function Assert-LibheifVersion {
  param([string[]]$Packages)
  $matching = @($Packages | Where-Object { $_ -match '^libheif:x64-windows-static-md\s+1\.23\.7(?:#\d+)?\s' })
  if ($matching.Count -ne 1) { throw 'Expected exactly one installed libheif 1.23.7 x64-windows-static-md port' }
  Write-Host "Verified $($matching[0])"
}

$tesseractCacheHit = $env:TESSERACT_CACHE_HIT -eq "true"
$llvmCacheHit = $env:LLVM_CACHE_HIT -eq "true"
. "$PSScriptRoot/ensure-tesseract-windows.ps1"
. "$PSScriptRoot/ensure-build-tools-windows.ps1"
$cmakeCacheHit = $env:CMAKE_CACHE_HIT -eq "true"
$cmakeInstalled = $false

Write-Host "Cache status:"
Write-Host "  TESSERACT_CACHE_HIT: $env:TESSERACT_CACHE_HIT (evaluated: $tesseractCacheHit)"
Write-Host "  LLVM_CACHE_HIT: $env:LLVM_CACHE_HIT (evaluated: $llvmCacheHit)"
Write-Host "  CMAKE_CACHE_HIT: $env:CMAKE_CACHE_HIT (evaluated: $cmakeCacheHit)"
Write-Host ""
try {
  & cmake --version 2>$null
  Write-Host "✓ CMake already installed"
  $cmakeInstalled = $true
}
catch {
  Write-Host "CMake not found, will attempt to install"
}

Ensure-BuildTool -Package 'llvm' -Executable 'C:\Program Files\LLVM\bin\clang.exe' -Version '23.1.3'

$libheifCacheHit = $env:LIBHEIF_CACHE_HIT -eq "true"
if (-not $libheifCacheHit) {
  Write-Host "libheif/boost/zlib cache miss, installing via vcpkg..."
  $vcpkgRoot = 'C:\xberg-vcpkg-5363acb7'
  $vcpkgExe = Join-Path $vcpkgRoot "vcpkg.exe"
  if (-not (Test-Path (Join-Path $vcpkgRoot '.git'))) {
    & git clone --filter=blob:none --no-checkout https://github.com/microsoft/vcpkg.git $vcpkgRoot
    if ($LASTEXITCODE -ne 0) { throw 'Failed to clone the dedicated vcpkg root' }
  }
  $baseline = '5363acb796e576328288fae60c25304e227e9839'
  & git -C $vcpkgRoot fetch --depth=1 origin $baseline
  if ($LASTEXITCODE -ne 0) { throw 'Failed to fetch the verified vcpkg baseline' }
  & git -C $vcpkgRoot checkout --detach $baseline
  if ($LASTEXITCODE -ne 0) { throw 'Failed to select the verified vcpkg baseline' }
  & (Join-Path $vcpkgRoot 'bootstrap-vcpkg.bat') -disableMetrics
  if ($LASTEXITCODE -ne 0) { throw 'Failed to bootstrap the verified vcpkg baseline' }
  if (-not (Retry-Command { & $vcpkgExe install "libheif[aom]:x64-windows-static-md" --recurse } -MaxAttempts 2)) {
    throw "Failed to install libheif via the verified vcpkg baseline"
  }
  else {
    Write-Host "✓ libheif installed via vcpkg (x64-windows-static-md)"
  }
  # boost-spirit + boost-serialization (header-only, used by librevenge/libwpd:
  # RVNGBinaryData.cpp needs boost/archive/iterators/*) and zlib (linked
  # by librevenge's zip stream) — needed by xberg-libwpd's build.rs.
  if (-not (Retry-Command { & $vcpkgExe install "boost-spirit:x64-windows-static-md" "boost-serialization:x64-windows-static-md" "zlib:x64-windows-static-md" --recurse } -MaxAttempts 2)) {
    throw "Failed to install boost/zlib via the verified vcpkg baseline"
  }
  else {
    Write-Host "✓ boost-spirit + boost-serialization + zlib installed via vcpkg (x64-windows-static-md)"
  }
  Add-Content -Path $env:GITHUB_ENV -Value "VCPKG_ROOT=$vcpkgRoot"
  Add-Content -Path $env:GITHUB_ENV -Value "VCPKG_INSTALLATION_ROOT=$vcpkgRoot"
}
else {
  Write-Host "✓ libheif/boost/zlib found in cache"
  $vcpkgRoot = 'C:\xberg-vcpkg-5363acb7'
  Add-Content -Path $env:GITHUB_ENV -Value "VCPKG_ROOT=$vcpkgRoot"
  Add-Content -Path $env:GITHUB_ENV -Value "VCPKG_INSTALLATION_ROOT=$vcpkgRoot"
}

$actualBaseline = & git -C $vcpkgRoot rev-parse HEAD
if ($LASTEXITCODE -ne 0 -or $actualBaseline -ne '5363acb796e576328288fae60c25304e227e9839') {
  throw 'The cached vcpkg checkout does not match its verified baseline'
}
$vcpkgExe = Join-Path $vcpkgRoot 'vcpkg.exe'
$heifPackages = @(& $vcpkgExe list 'libheif:x64-windows-static-md')
if ($LASTEXITCODE -ne 0) { throw 'Failed to query the installed libheif port' }
Assert-LibheifVersion -Packages $heifPackages

# Neither branch above proves zlib is on disk: the install path prints its "✓" whenever the
# vcpkg call merely does not throw, and the cache path prints one unconditionally. When the port
# is actually missing, nothing notices until `xberg-libwpd`'s build.rs probe finds no archive and
# emits its deliberate loud-failure stem -- surfacing ~45 minutes later, in a different step, as
# `rust-lld: could not open 'zlibstatic.lib'`, which reads like a wrong-library-name bug rather
# than a missing dependency. That is how the Elixir NIF Windows leg failed for v1.1.0 and v1.1.1,
# taking the Hex publish down with it. Check the artifact itself, using the same stem list
# build.rs probes (crates/xberg-libwpd/build.rs ZLIB_RELEASE_STEMS). ~keep
$zlibLibDir = Join-Path $vcpkgRoot "installed\x64-windows-static-md\lib"
$zlibStems = @("zlibstatic", "zlib", "zs", "z")
$zlibFound = $zlibStems | Where-Object { Test-Path (Join-Path $zlibLibDir "$_.lib") } | Select-Object -First 1
if ($zlibFound) {
  Write-Host "✓ zlib archive verified: $(Join-Path $zlibLibDir "$zlibFound.lib")"
}
else {
  Write-Host "::error::no zlib archive ($($zlibStems -join '/')).lib found in $zlibLibDir -- vcpkg reported success but the port is not on disk; xberg-libwpd cannot link"
  exit 1
}

foreach ($header in @('boost/spirit/include/qi.hpp', 'boost/archive/iterators/base64_from_binary.hpp')) {
  $path = Join-Path $vcpkgRoot "installed/x64-windows-static-md/include/$header"
  if (-not (Test-Path -LiteralPath $path)) { throw "Required Boost header is missing: $path" }
}

Write-Host "Installing PHP..."
$phpInstalled = $false
try {
  & php --version 2>$null
  Write-Host "✓ PHP already installed"
  $phpInstalled = $true
}
catch {
  Write-Host "PHP not found, installing via Chocolatey..."
  if (-not (Retry-Command { choco install -y php --no-progress } -MaxAttempts 3)) {
    Write-Host "::warning::Failed to install PHP via Chocolatey, will rely on shivammathur/setup-php action"
  }
  else {
    Write-Host "✓ PHP installed via Chocolatey"
    $phpInstalled = $true
  }
}

Ensure-BuildTool -Package 'cmake' -Executable 'C:\Program Files\CMake\bin\cmake.exe' -Version '4.4.4'

Ensure-Tesseract -Executable 'C:\Program Files\Tesseract-OCR\tesseract.exe' `
  -SourceArchive (Join-Path $env:TEMP 'xberg-tesseract-5.5.4.zip') -VcpkgRoot $vcpkgRoot
Ensure-TesseractData -Directory 'C:\Program Files\Tesseract-OCR\tessdata'

Write-Host "Configuring PATH and environment variables..."
$paths = @(
  "C:\Program Files\CMake\bin",
  "C:\Program Files\Tesseract-OCR",
  "C:\Program Files\LLVM\bin",
  "C:\tools\php",
  "C:\Program Files\PHP"
)

foreach ($path in $paths) {
  if (Test-Path $path) {
    Write-Host "  Adding to PATH: $path"
    Write-Output $path | Out-File -FilePath $env:GITHUB_PATH -Encoding utf8 -Append
    $env:PATH = "$path;$env:PATH"
  }
  else {
    Write-Host "  Path not found (skipping): $path"
  }
}

$tesseractPath = "C:\Program Files\Tesseract-OCR"
if (Test-Path $tesseractPath) {
  $tessdataPath = "$tesseractPath\tessdata"
  if (Test-Path $tessdataPath) {
    Write-Host "  Setting TESSDATA_PREFIX for tests: $tessdataPath"
    Add-Content -Path $env:GITHUB_ENV -Value "TESSDATA_PREFIX=$tessdataPath"
    $env:TESSDATA_PREFIX = $tessdataPath
  }
}

Write-Host "::endgroup::"

Write-Host "::group::Verifying Windows installations"

Write-Host "Tesseract (optional for build):"
try {
  $tesseractCmd = Get-Command tesseract -ErrorAction Stop
  $tesseractPath = $tesseractCmd.Path
  Write-Host "  Found at: $tesseractPath"
  Write-Host "  Command type: $($tesseractCmd.CommandType)"

  $tesseractDir = Split-Path -Parent $tesseractPath
  Write-Host "  Installation directory: $tesseractDir"

  $tessdataPath = Join-Path $tesseractDir "tessdata"
  if (Test-Path $tessdataPath) {
    Write-Host "  tessdata directory: $tessdataPath"
    Write-Host "  Available language files:"
    Get-ChildItem "$tessdataPath\*.traineddata" -ErrorAction SilentlyContinue | ForEach-Object {
      Write-Host "    - $($_.Name)"
    }
  }
  else {
    Write-Host "  tessdata directory not found at: $tessdataPath"
  }

  try {
    $version = & tesseract --version 2>&1
    Write-Host "  Version output: $version"
    Write-Host "✓ Tesseract available and working"

    Write-Host ""
    Write-Host "Available Tesseract languages:"
    & tesseract --list-langs 2>&1 | ForEach-Object { Write-Host "  $_" }
  }
  catch {
    Write-Host "⚠ Warning: Tesseract found but failed to run: $($_.Exception.Message)"
  }

  if (Test-Path $tessdataPath) {
    Write-Host ""
    Write-Host "Setting TESSDATA_PREFIX environment variable..."
    Add-Content -Path $env:GITHUB_ENV -Value "TESSDATA_PREFIX=$tessdataPath"
    Write-Host "✓ Set TESSDATA_PREFIX=$tessdataPath in GITHUB_ENV"
    $env:TESSDATA_PREFIX = $tessdataPath
  }
}
catch {
  Write-Host "⚠ Tesseract not found on PATH (not required for build)"
  Write-Host "  Error details: $($_.Exception.Message)"
  Write-Host "  Searching common installation locations..."

  $commonPaths = @(
    "C:\Program Files\Tesseract-OCR\tesseract.exe",
    "C:\Program Files (x86)\Tesseract-OCR\tesseract.exe",
    "${env:ProgramFiles}\Tesseract-OCR\tesseract.exe",
    "${env:ProgramFiles(x86)}\Tesseract-OCR\tesseract.exe"
  )

  $found = $false
  foreach ($path in $commonPaths) {
    if (Test-Path $path) {
      Write-Host "  Found Tesseract at: $path (not on PATH)"
      $tesseractDir = Split-Path -Parent $path
      $tessdataPath = Join-Path $tesseractDir "tessdata"
      if (Test-Path $tessdataPath) {
        Write-Host "  Found tessdata at: $tessdataPath"
        Add-Content -Path $env:GITHUB_ENV -Value "TESSDATA_PREFIX=$tessdataPath"
        Write-Host "✓ Set TESSDATA_PREFIX=$tessdataPath in GITHUB_ENV"
        $env:TESSDATA_PREFIX = $tessdataPath
      }
      $found = $true
      break
    }
  }

  if (-not $found) {
    Write-Host "  Tesseract not found in common locations"
  }
}

Write-Host ""
Write-Host "CMake:"
try {
  & cmake --version
  Write-Host "✓ CMake available"
  $cmakePath = (Get-Command cmake -ErrorAction Stop).Source
  if ($cmakePath) {
    Add-Content -Path $env:GITHUB_ENV -Value "CMAKE=$cmakePath"
    Write-Host "✓ Set CMAKE=$cmakePath in GITHUB_ENV"
  }
}
catch {
  Write-Host "::error::CMake not found after installation"
  throw "CMake verification failed"
}

Write-Host ""
Write-Host "Clang:"
try {
  & clang --version
  Write-Host "✓ Clang available"
}
catch {
  Write-Host "⚠ Warning: Clang not currently available on PATH"
}

Write-Host ""
Write-Host "PHP:"
try {
  & php --version
  Write-Host "✓ PHP available"
}
catch {
  Write-Host "⚠ Warning: PHP not currently available on PATH (will be set up by shivammathur/setup-php action)"
}

Write-Host "::endgroup::"
