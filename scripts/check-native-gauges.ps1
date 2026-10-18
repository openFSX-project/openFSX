param(
    [Parameter(Mandatory=$true)][string]$FsxRoot,
    [Parameter(Mandatory=$true)][string]$Output
)
$ErrorActionPreference = 'Stop'
$gaugeCheckRoot = Split-Path $PSScriptRoot -Parent
$gaugeCheckHelper = Join-Path $gaugeCheckRoot 'target/fsx-gauge-host.exe'
if (-not (Test-Path -LiteralPath $gaugeCheckHelper)) { throw 'Build the independent x86 helper with scripts/build-gauge-host.ps1 first.' }
$gaugeCheckInstallation = (Resolve-Path -LiteralPath $FsxRoot).Path
$gaugeCheckOutput = $ExecutionContext.SessionState.Path.GetUnresolvedProviderPathFromPSPath($Output)
$gaugeCheckStream = [IO.File]::Open($gaugeCheckOutput, [IO.FileMode]::CreateNew, [IO.FileAccess]::Write, [IO.FileShare]::Read)
$gaugeCheckWriter = [IO.StreamWriter]::new($gaugeCheckStream, [Text.UTF8Encoding]::new($false))
try {
    $gaugeCheckProbes = @(
        @('Cessna.dll','Altimeter'),
        @('Cessna172.dll','Airspeed'),
        @('Cessna182s.dll','Oil_Press_Temp'),
        @('Bell_206B.dll','Attitude'),
        @('Bendix_king_radio.dll','Bendix-King Radio Audio')
    )
    foreach ($gaugeCheckProbe in $gaugeCheckProbes) {
        $gaugeCheckLine = & $gaugeCheckHelper --probe $gaugeCheckInstallation @gaugeCheckProbe
        if ($LASTEXITCODE -ne 0) { throw "Stock native library probe failed: $gaugeCheckProbe" }
        if (@($gaugeCheckLine).Count -ne 1) { throw 'Expected one native probe JSON record.' }
        $gaugeCheckRecord = $gaugeCheckLine | ConvertFrom-Json
        if ($gaugeCheckRecord.schema -ne 1 -or $gaugeCheckRecord.source -ne 'native-gauge-probe' -or $gaugeCheckRecord.mode -ne 'probe' -or $gaugeCheckRecord.pointer_bits -ne 32 -or $gaugeCheckRecord.module_init_called -or $gaugeCheckRecord.module_deinit_called -or $gaugeCheckRecord.panel_services) { throw 'Invalid native probe record.' }
        $gaugeCheckWriter.WriteLine($gaugeCheckLine)
    }
    $gaugeCheckSamples = @(
        @('Cessna172.dll','Airspeed','1001',@('0','75','200')),
        @('Cessna172.dll','Tachometer','5023',@('0','1800','2700')),
        @('Cessna.dll','Vertical_Speed','5047',@('-256','0','256')),
        @('Cessna.dll','Altimeter','1031',@('0','500','1000')),
        @('Cessna.dll','Altimeter','1032',@('0','500','1000')),
        @('Cessna.dll','Altimeter','1033',@('0','500','1000'))
    )
    foreach ($gaugeCheckSample in $gaugeCheckSamples) {
        $gaugeCheckValues = @()
        foreach ($gaugeCheckInput in $gaugeCheckSample[3]) {
            $gaugeCheckArguments = @('--sample',$gaugeCheckInstallation,$gaugeCheckSample[0],$gaugeCheckSample[1],$gaugeCheckSample[2],$gaugeCheckInput)
            $gaugeCheckLine = & $gaugeCheckHelper @gaugeCheckArguments
            if ($LASTEXITCODE -ne 0) { throw "Stock native callback failed: $gaugeCheckArguments" }
            if (@($gaugeCheckLine).Count -ne 1) { throw 'Expected one native callback JSON record.' }
            $gaugeCheckRecord = $gaugeCheckLine | ConvertFrom-Json
            if ($gaugeCheckRecord.schema -ne 1 -or $gaugeCheckRecord.source -ne 'native-gauge-probe' -or $gaugeCheckRecord.mode -ne 'sample' -or $gaugeCheckRecord.pointer_bits -ne 32 -or -not $gaugeCheckRecord.module_init_called -or -not $gaugeCheckRecord.module_deinit_called -or $gaugeCheckRecord.panel_services -or $gaugeCheckRecord.sample.resource -ne [int]$gaugeCheckSample[2]) { throw 'Invalid native callback record.' }
            $gaugeCheckResult = [double]$gaugeCheckRecord.sample.callback_result
            if ($gaugeCheckRecord.library -ne $gaugeCheckSample[0] -or $gaugeCheckRecord.gauge -ne $gaugeCheckSample[1] -or $gaugeCheckRecord.sample.raw_input -ne [double]$gaugeCheckInput) { throw 'Native callback response does not match the request.' }
            if ([double]::IsNaN($gaugeCheckResult) -or [double]::IsInfinity($gaugeCheckResult)) { throw 'Non-finite native callback result.' }
            $gaugeCheckValues += $gaugeCheckResult
            $gaugeCheckWriter.WriteLine($gaugeCheckLine)
        }
        if (($gaugeCheckValues | Select-Object -Unique).Count -lt 2) { throw 'Native callback did not respond to distinct raw input values.' }
        $gaugeCheckFormatted = $gaugeCheckValues | ForEach-Object { $_.ToString('G17',[Globalization.CultureInfo]::InvariantCulture) }
        Write-Output "$($gaugeCheckSample[0])!$($gaugeCheckSample[1]) resource $($gaugeCheckSample[2]): $($gaugeCheckFormatted -join ', ')"
    }
} finally {
    $gaugeCheckWriter.Dispose()
}
Write-Output "PASS: five loaded stock library probes and 18 isolated needle-update samples. Raw inputs are inspection values, not recorded FSX flight data. Saved $gaugeCheckOutput"
