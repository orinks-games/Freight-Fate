# Windows portable runtime packaging

Career 1.9 Windows archives carry the Visual C++ runtime beside
`FreightFate.exe`, so extracting a snapshot does not require an administrator
or a separate runtime installer. The updater already copies root-level DLLs.
This changes packaging only; the game and its gameplay tests remain Rust.

The release tooling (`crates/ff-tools/src/windows_runtime.rs`) collects the
complete x64 `Microsoft.VC*.CRT` directory from the installed Visual Studio
C++ toolchain. It first uses
`VCToolsRedistDir`, then `VCINSTALLDIR` / `VSINSTALLDIR`, then Visual Studio's
`vswhere.exe` for shells without a developer environment. Version directories
are ordered numerically. An explicitly configured but incomplete redist fails
instead of silently choosing a different toolchain. A local builder can set
`VCToolsRedistDir` to `VC/Redist/MSVC/<version>` if automatic discovery fails.

The source must contain `vcruntime140.dll`, `vcruntime140_1.dll`, and
`msvcp140.dll`; all DLLs in that CRT directory are copied together so companion
libraries such as `msvcp140_atomic_wait.dll` use the same toolchain release.
Copies left by Cargo are replaced. No files are sourced from System32,
Python environments, or individual DLL download sites. Updating the build
toolchain updates the bundled runtime on subsequent builds. The build log
records the source directory and filenames.

The staging and final payload checks parse the game and all bundled DLLs,
including plugins and the runtime itself. They reject wrong-architecture or
malformed binaries and require every normal or delayed Visual C++ import to
exist beside the executable, comparing names without case sensitivity. This
is a VC dependency check, not a general Windows loader simulation; Windows
system DLLs and API sets are provided by the supported operating system.

`cargo test -p ff-tools windows_runtime` runs the packaging tests; on
Windows it also stages and audits the installed Visual Studio CRT, which is
how CI's required Windows test check covers it. The existing packaged smoke
test still runs on the Windows builder. Release verification should also
extract the resulting archive into a fresh supported Windows installation
without the Visual C++ redistributable installed, start the game through its
headless smoke interface, and manually confirm sound and screen-reader speech.
Do not uninstall system runtimes from a shared developer machine to do this.

Microsoft documents redistributable source directories and application-local
deployment in [Redistributing Visual C++ files](https://learn.microsoft.com/en-us/cpp/windows/redistributing-visual-cpp-files).
Redistribution remains subject to the build toolchain's license and REDIST
list. Only the release CRT directory is collected; debug runtimes are not
redistributable. PE import structures follow the [PE format specification](https://learn.microsoft.com/en-us/windows/win32/debug/pe-format)
and the [delay-load helper documentation](https://learn.microsoft.com/en-us/cpp/build/reference/understanding-the-helper-function).
