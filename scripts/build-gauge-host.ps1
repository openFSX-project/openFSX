$ErrorActionPreference = 'Stop'
$gaugeRoot = Split-Path $PSScriptRoot -Parent
$gaugeVswhere = Join-Path ${env:ProgramFiles(x86)} 'Microsoft Visual Studio/Installer/vswhere.exe'
if (-not (Test-Path -LiteralPath $gaugeVswhere)) { throw 'Visual Studio C++ Build Tools are required.' }
$gaugeInstallation = & $gaugeVswhere -latest -products '*' -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath
if (-not $gaugeInstallation) { throw 'No installed x86 C++ toolchain found.' }
$gaugeVcVersion = (Get-Content -LiteralPath (Join-Path $gaugeInstallation 'VC/Auxiliary/Build/Microsoft.VCToolsVersion.default.txt') -Raw).Trim()
$gaugeVc = Join-Path $gaugeInstallation "VC/Tools/MSVC/$gaugeVcVersion"
$gaugeKit = Join-Path ${env:ProgramFiles(x86)} 'Windows Kits/10'
$gaugeSdk = Get-ChildItem -LiteralPath (Join-Path $gaugeKit 'Include') -Directory | Where-Object { Test-Path -LiteralPath (Join-Path $_.FullName 'um/windows.h') } | Sort-Object { [version]$_.Name } -Descending | Select-Object -First 1
if (-not $gaugeSdk) { throw 'A Windows SDK is required.' }
$gaugeTarget = Join-Path $gaugeRoot 'target'
New-Item -ItemType Directory -Path $gaugeTarget -Force | Out-Null
$gaugeOutput = Join-Path $gaugeTarget 'fsx-gauge-host.exe'
$gaugeArguments = @('/nologo','/std:c++20','/EHsc','/O2','/W4','/MT','/permissive-',"/I$(Join-Path $gaugeVc 'include')")
foreach ($gaugeInclude in @('ucrt','shared','um')) { $gaugeArguments += "/I$(Join-Path $gaugeSdk.FullName $gaugeInclude)" }
$gaugeArguments += @("/Fo$(Join-Path $gaugeTarget 'fsx-gauge-host.obj')", "/Fe$gaugeOutput", (Join-Path $gaugeRoot 'tools/fsx-gauge-host/main.cpp'), '/link', '/MACHINE:X86', "/LIBPATH:$(Join-Path $gaugeVc 'lib/x86')")
foreach ($gaugeLibrary in @('ucrt','um')) { $gaugeArguments += "/LIBPATH:$(Join-Path $gaugeKit "Lib/$($gaugeSdk.Name)/$gaugeLibrary/x86")" }
$gaugeArguments += @('kernel32.lib','psapi.lib')
& (Join-Path $gaugeVc 'bin/Hostx64/x86/cl.exe') @gaugeArguments
if ($LASTEXITCODE -ne 0) { throw 'Native x86 gauge helper compilation failed.' }
$gaugeSelfTest = & $gaugeOutput --self-test
if ($LASTEXITCODE -ne 0) { throw 'Native gauge helper self-test failed.' }
$gaugeDecoded = $gaugeSelfTest | ConvertFrom-Json
if ($gaugeDecoded.schema -ne 1 -or $gaugeDecoded.source -ne 'synthetic' -or $gaugeDecoded.pointer_bits -ne 32 -or $gaugeDecoded.stdcall_result -ne 11 -or -not $gaugeDecoded.exception_contained -or -not $gaugeDecoded.string_guard_verified -or -not $gaugeDecoded.gauge_notifications_verified -or -not $gaugeDecoded.mouse_context_verified -or -not $gaugeDecoded.event_abi_verified -or -not $gaugeDecoded.clock_commands_verified -or -not $gaugeDecoded.ansi_text_verified -or $gaugeDecoded.game_loaded) { throw 'Unexpected native gauge helper self-test protocol.' }
Write-Output $gaugeSelfTest
Write-Output 'Built independent target/fsx-gauge-host.exe. Self-test uses original synthetic code, no FSX DLL or process.'
