param([Parameter(Mandatory=$true)][string]$OutputDirectory)
$ErrorActionPreference = 'Stop'
$referenceCheckRoot = Split-Path $PSScriptRoot -Parent
$referenceCheckOutput = $ExecutionContext.SessionState.Path.GetUnresolvedProviderPathFromPSPath($OutputDirectory)
if (Test-Path -LiteralPath $referenceCheckOutput) { throw 'Choose a new output directory for reference protocol checks.' }
$referenceCheckFixture = Join-Path $referenceCheckRoot 'target/reference-instruments-synthetic.ndjson'
if (-not (Test-Path -LiteralPath $referenceCheckFixture)) { throw 'Run scripts/build-reference.ps1 first.' }
New-Item -ItemType Directory -Path $referenceCheckOutput | Out-Null
& cargo build --locked -p fsx-app --features viewer --example reference-gauge-replay
if ($LASTEXITCODE -ne 0) { throw 'Reference replay validator build failed.' }
$referenceCheckValidator = Join-Path $referenceCheckRoot 'target/debug/examples/reference-gauge-replay.exe'
$referenceCheckBase = Get-Content -LiteralPath $referenceCheckFixture -Raw
$referenceCheckReport = & $referenceCheckValidator --validate $referenceCheckFixture
if ($LASTEXITCODE -ne 0) { throw 'The Rust reader rejected the C# schema-2 fixture.' }
$referenceCheckDecoded = $referenceCheckReport | ConvertFrom-Json
if ($referenceCheckDecoded.samples -ne 1 -or $referenceCheckDecoded.game_loaded -or $referenceCheckDecoded.recorded_source -ne 'synthetic' -or $referenceCheckDecoded.first_native_inputs[0] -ne 90 -or $referenceCheckDecoded.first_native_inputs[1] -ne 1000 -or [Math]::Abs($referenceCheckDecoded.first_native_inputs[2] - 780.288) -gt 1e-9 -or $referenceCheckDecoded.first_native_inputs[3] -ne -10 -or $referenceCheckDecoded.first_native_inputs[4] -ne -30 -or $referenceCheckDecoded.first_native_inputs[5] -ne 90) { throw 'C#/Rust instrument schema or native-unit mapping differs from the original synthetic fixture.' }
[IO.File]::WriteAllText((Join-Path $referenceCheckOutput 'single-report.json'),$referenceCheckReport,[Text.UTF8Encoding]::new($false))
$referenceCheckSequence = @()
for ($referenceCheckIndex=0; $referenceCheckIndex -lt 5; $referenceCheckIndex++) {
    $referenceCheckRow = $referenceCheckBase | ConvertFrom-Json
    $referenceCheckRow.sample_index = $referenceCheckIndex
    $referenceCheckRow.elapsed_seconds = 0.25 + $referenceCheckIndex * 0.25
    $referenceCheckDelta = @(0,0,0.5,-10,-9.75)[$referenceCheckIndex]
    $referenceCheckRow.instruments.sim_time_seconds += $referenceCheckDelta
    if ($referenceCheckIndex -eq 2 -or $referenceCheckIndex -eq 4) {
        $referenceCheckRow.instruments.ias_knots = 110
        $referenceCheckRow.instruments.indicated_altitude_ft = 1500
        $referenceCheckRow.instruments.gyro_pitch_deg = -5
        $referenceCheckRow.instruments.gyro_bank_deg = -10
        $referenceCheckRow.instruments.gyro_heading_deg = 270
    }
    $referenceCheckSequence += $referenceCheckRow | ConvertTo-Json -Compress -Depth 8
}
$referenceCheckSequencePath = Join-Path $referenceCheckOutput 'sequence-synthetic.ndjson'
[IO.File]::WriteAllText($referenceCheckSequencePath,($referenceCheckSequence -join "`n") + "`n",[Text.UTF8Encoding]::new($false))
$referenceCheckReport = & $referenceCheckValidator --validate $referenceCheckSequencePath
if ($LASTEXITCODE -ne 0) { throw 'The Rust reader rejected the synthetic instrument sequence.' }
$referenceCheckDecoded = $referenceCheckReport | ConvertFrom-Json
if ($referenceCheckDecoded.samples -ne 5 -or $referenceCheckDecoded.unchanged_sim_clock_rows -ne 1 -or $referenceCheckDecoded.clock_rewinds -ne 1 -or $referenceCheckDecoded.unsupported_native_profile_rows -ne 0) { throw 'Synthetic pause/rewind sequence was decoded incorrectly.' }
[IO.File]::WriteAllText((Join-Path $referenceCheckOutput 'sequence-report.json'),$referenceCheckReport,[Text.UTF8Encoding]::new($false))
foreach ($referenceCheckMutation in @('object','source','index','field')) {
    $referenceCheckBad = $referenceCheckSequence[1] | ConvertFrom-Json
    switch ($referenceCheckMutation) {
        'object' { $referenceCheckBad.object_id = 99 }
        'source' { $referenceCheckBad.source = 'FSX SimConnect' }
        'index' { $referenceCheckBad.sample_index = 5 }
        'field' { $referenceCheckBad.instruments.PSObject.Properties.Remove('gyro_pitch_deg') }
    }
    $referenceCheckBadPath = Join-Path $referenceCheckOutput ("reject-$referenceCheckMutation.ndjson")
    [IO.File]::WriteAllText($referenceCheckBadPath,$referenceCheckSequence[0] + "`n" + ($referenceCheckBad | ConvertTo-Json -Compress -Depth 8) + "`n",[Text.UTF8Encoding]::new($false))
    # Windows PowerShell wraps redirected native stderr in ErrorRecord objects.
    # These four nonzero exits are expected validation results.
    $ErrorActionPreference = 'Continue'
    try {
        & $referenceCheckValidator --validate $referenceCheckBadPath 2> (Join-Path $referenceCheckOutput ("reject-$referenceCheckMutation.stderr.txt")) | Out-Null
    } finally { $ErrorActionPreference = 'Stop' }
    if ($LASTEXITCODE -eq 0) { throw "Malformed reference $referenceCheckMutation was accepted." }
}
Write-Output 'PASS: C# schema-2 fixture -> Rust reader -> exact native unit inputs; five synthetic rows, pause/rewind, and four malformed-sequence rejections. No FSX files or runtime loaded.'
exit 0
