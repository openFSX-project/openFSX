$ErrorActionPreference = 'Stop'
$terrainRoot = Split-Path $PSScriptRoot -Parent
$terrainVswhere = Join-Path ${env:ProgramFiles(x86)} 'Microsoft Visual Studio/Installer/vswhere.exe'
if (-not (Test-Path -LiteralPath $terrainVswhere)) { throw 'Visual Studio C++ Build Tools are required.' }
$terrainInstallation = & $terrainVswhere -latest -products '*' -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath
if (-not $terrainInstallation) { throw 'No installed x86 C++ toolchain found.' }
$terrainVcVersion = (Get-Content -LiteralPath (Join-Path $terrainInstallation 'VC/Auxiliary/Build/Microsoft.VCToolsVersion.default.txt') -Raw).Trim()
$terrainVc = Join-Path $terrainInstallation "VC/Tools/MSVC/$terrainVcVersion"
$terrainKit = Join-Path ${env:ProgramFiles(x86)} 'Windows Kits/10'
$terrainSdk = Get-ChildItem -LiteralPath (Join-Path $terrainKit 'Include') -Directory | Where-Object { Test-Path -LiteralPath (Join-Path $_.FullName 'um/windows.h') } | Sort-Object { [version]$_.Name } -Descending | Select-Object -First 1
if (-not $terrainSdk) { throw 'A Windows SDK is required.' }
$terrainTarget = Join-Path $terrainRoot 'target'
New-Item -ItemType Directory -Path $terrainTarget -Force | Out-Null
$terrainOutput = Join-Path $terrainTarget 'fsx-terrain-host.exe'
$terrainArguments = @('/nologo','/std:c++20','/EHsc','/O2','/W4','/MT','/permissive-',"/I$(Join-Path $terrainVc 'include')")
foreach ($terrainInclude in @('ucrt','shared','um')) { $terrainArguments += "/I$(Join-Path $terrainSdk.FullName $terrainInclude)" }
$terrainArguments += @("/Fo$(Join-Path $terrainTarget 'fsx-terrain-host.obj')", "/Fe$terrainOutput", (Join-Path $terrainRoot 'tools/fsx-terrain-host/main.cpp'), '/link', '/MACHINE:X86', "/LIBPATH:$(Join-Path $terrainVc 'lib/x86')")
foreach ($terrainLibrary in @('ucrt','um')) { $terrainArguments += "/LIBPATH:$(Join-Path $terrainKit "Lib/$($terrainSdk.Name)/$terrainLibrary/x86")" }
$terrainArguments += @('kernel32.lib')
& (Join-Path $terrainVc 'bin/Hostx64/x86/cl.exe') @terrainArguments
if ($LASTEXITCODE -ne 0) { throw 'Native x86 terrain helper compilation failed.' }
Write-Output 'Built independent target/fsx-terrain-host.exe. It loads the installed terrain.dll only when run with --decode.'
