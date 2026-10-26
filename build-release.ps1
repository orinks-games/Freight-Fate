# Build a standalone Freight Fate copy on Windows.
#
# Checks the prerequisites, installs the pinned Rust toolchain, then hands
# over to `cargo tools build-release`, which fetches the music pack, builds
# the game, bakes the world, smokes the packaged build and writes the
# portable zip to dist/. Extra arguments (for example --tag or
# --cargo-target-dir) pass through to the builder.
#
# Prism (the screen-reader and speech library) is compiled from C++ source by
# the build, so besides Rust it needs the Visual Studio Build Tools with the
# "Desktop development with C++" workload PLUS the optional C++ ATL component:
# four of its speech backends include <atlbase.h>. Without ATL, cargo fails
# deep inside prism-sys with "Cannot open include file: 'atlbase.h'", so it is
# checked for up front here with an actionable message.

# Native tools write progress to stderr; under Windows PowerShell 5.1 with
# ErrorActionPreference = Stop that becomes a terminating error whenever the
# output is redirected or piped (for example into Tee-Object). Failures are
# reported through explicit throws and exit-code checks instead.
$ErrorActionPreference = "Continue"
$readme = "See README.md under Build a standalone copy."

Set-Location $PSScriptRoot

foreach ($tool in @("git", "rustup", "rustc", "cargo", "cmake")) {
    if (-not (Get-Command $tool -ErrorAction SilentlyContinue)) {
        throw "Freight Fate needs $tool before it can build. $readme"
    }
}

function Find-VisualStudio {
    $vswhere = Join-Path ${env:ProgramFiles(x86)} "Microsoft Visual Studio\Installer\vswhere.exe"
    if (-not (Test-Path $vswhere)) { return $null }
    $found = & $vswhere -latest -products * `
        -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 `
        -property installationPath 2>$null
    if ($LASTEXITCODE -ne 0 -or -not $found) { return $null }
    return ($found | Select-Object -First 1).Trim()
}

$vsRoot = Find-VisualStudio
if (-not $vsRoot) {
    throw @"
Freight Fate needs the Visual Studio Build Tools with the "Desktop development with C++" workload and the C++ ATL component. Install them with:
  winget install Microsoft.VisualStudio.2022.BuildTools --override "--wait --passive --norestart --add Microsoft.VisualStudio.Workload.VCTools --add Microsoft.VisualStudio.Component.VC.ATL --includeRecommended"
then open a new terminal and run this script again. $readme
"@
}

$atlHeader = Get-ChildItem -Path (Join-Path $vsRoot "VC\Tools\MSVC") -Filter atlbase.h -Recurse -ErrorAction SilentlyContinue |
    Where-Object { $_.FullName -like "*\atlmfc\include\*" } |
    Select-Object -First 1
if (-not $atlHeader) {
    $installer = Join-Path ${env:ProgramFiles(x86)} "Microsoft Visual Studio\Installer\vs_installer.exe"
    throw @"
Freight Fate needs the C++ ATL component of the Visual Studio Build Tools (Prism's speech backends include atlbase.h), and the Visual Studio at
  $vsRoot
does not have it. Add it with:
  & "$installer" modify --installPath "$vsRoot" --add Microsoft.VisualStudio.Component.VC.ATL --passive --norestart
then open a new terminal and run this script again. $readme
"@
}

# rust-toolchain.toml pins the compiler; this installs it (with rustfmt and
# Clippy) when it is not there yet, instead of failing inside cargo.
rustup show | Out-Null
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }

cargo tools build-release --smoke @args
exit $LASTEXITCODE
