$ErrorActionPreference = 'Stop'
$referenceRoot = Split-Path $PSScriptRoot -Parent
$referenceCompiler = Join-Path $env:WINDIR 'Microsoft.NET/Framework/v4.0.30319/csc.exe'
if (-not (Test-Path -LiteralPath $referenceCompiler)) { throw 'The Windows .NET Framework C# compiler is required.' }
$referenceTarget = Join-Path $referenceRoot 'target'
New-Item -ItemType Directory -Path $referenceTarget -Force | Out-Null
$referenceOutput = Join-Path $referenceTarget 'fsx-reference.exe'
$referenceSource = Join-Path $referenceRoot 'tools/fsx-reference/Program.cs'
& $referenceCompiler /nologo /platform:x86 /optimize+ "/out:$referenceOutput" $referenceSource
if ($LASTEXITCODE -ne 0) { throw 'Reference helper compilation failed.' }
& $referenceOutput --self-test
if ($LASTEXITCODE -ne 0) { throw 'Reference protocol self-test failed.' }
$referenceFixture = & $referenceOutput --synthetic-sample
if ($LASTEXITCODE -ne 0) { throw 'Synthetic reference encoding failed.' }
$referenceDecoded = $referenceFixture | ConvertFrom-Json
if ($referenceDecoded.schema -ne 1 -or $referenceDecoded.source -ne 'synthetic' -or $referenceDecoded.values.on_ground -ne 0) { throw 'Synthetic sample JSON does not match the documented schema.' }
[IO.File]::WriteAllText((Join-Path $referenceTarget 'reference-synthetic.ndjson'), $referenceFixture + "`n", [Text.UTF8Encoding]::new($false))
Write-Output "Built $referenceOutput. This separate helper reads an already-running local FSX through its installed x86 client."
