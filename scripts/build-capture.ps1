$ErrorActionPreference = 'Stop'
$captureRoot = Split-Path $PSScriptRoot -Parent
$captureVswhere = Join-Path ${env:ProgramFiles(x86)} 'Microsoft Visual Studio/Installer/vswhere.exe'
if (-not (Test-Path -LiteralPath $captureVswhere)) { throw 'Visual Studio C++ Build Tools are required.' }
$captureInstallation = & $captureVswhere -latest -products '*' -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath
if (-not $captureInstallation) { throw 'No installed x64 C++ toolchain found.' }
$captureVcVersion = (Get-Content -LiteralPath (Join-Path $captureInstallation 'VC/Auxiliary/Build/Microsoft.VCToolsVersion.default.txt') -Raw).Trim()
$captureVc = Join-Path $captureInstallation "VC/Tools/MSVC/$captureVcVersion"
$captureKit = Join-Path ${env:ProgramFiles(x86)} 'Windows Kits/10'
$captureSdk = Get-ChildItem -LiteralPath (Join-Path $captureKit 'Include') -Directory | Where-Object { Test-Path -LiteralPath (Join-Path $_.FullName 'cppwinrt/winrt/Windows.Graphics.Capture.h') } | Sort-Object { [version]$_.Name } -Descending | Select-Object -First 1
if (-not $captureSdk) { throw 'A Windows SDK with C++/WinRT capture headers is required.' }
$captureTarget = Join-Path $captureRoot 'target'
New-Item -ItemType Directory -Path $captureTarget -Force | Out-Null
$captureArguments = @('/nologo','/std:c++20','/EHsc','/O2','/W4','/permissive-',"/I$(Join-Path $captureVc 'include')")
foreach ($captureInclude in @('ucrt','shared','um','winrt','cppwinrt')) { $captureArguments += "/I$(Join-Path $captureSdk.FullName $captureInclude)" }
$captureArguments += @("/Fo$(Join-Path $captureTarget 'fsx-capture.obj')", "/Fe$(Join-Path $captureTarget 'fsx-capture.exe')", (Join-Path $captureRoot 'tools/fsx-capture/main.cpp'), '/link', "/LIBPATH:$(Join-Path $captureVc 'lib/x64')")
foreach ($captureLibrary in @('ucrt','um')) { $captureArguments += "/LIBPATH:$(Join-Path $captureKit "Lib/$($captureSdk.Name)/$captureLibrary/x64")" }
$captureArguments += @('d3d11.lib','dxgi.lib','windowsapp.lib','user32.lib')
& (Join-Path $captureVc 'bin/Hostx64/x64/cl.exe') @captureArguments
if ($LASTEXITCODE -ne 0) { throw 'Background capture compilation failed.' }
& (Join-Path $captureTarget 'fsx-capture.exe') --self-test
if ($LASTEXITCODE -ne 0) { throw 'Background capture self-test failed.' }
Write-Output 'Built independent target/fsx-capture.exe. No game, input or desktop activation is used by the build/test.'
