#requires -Version 7.0
param(
    [Parameter(Mandatory = $true)][string]$Executable,
    [string]$ReportPath,
    [ValidateRange(1.0, 4.0)][float]$Scale = 1.0,
    [ValidateRange(0, 2000)][int]$LatencySamples = 0
)

$ErrorActionPreference = 'Stop'
if ($LatencySamples -gt 0 -and $LatencySamples -lt 10) { throw 'LatencySamples must be zero or between 10 and 2000' }
$checkDirectory = Join-Path ([IO.Path]::GetTempPath()) ('whostty-check-' + [guid]::NewGuid().ToString('N'))
$null = New-Item -ItemType Directory -Path $checkDirectory
$nativeCheckProcess = $null
try {
    $startInfo = [Diagnostics.ProcessStartInfo]::new()
    $startInfo.FileName = (Resolve-Path -LiteralPath $Executable).ProviderPath
    $startInfo.ArgumentList.Add('--check')
    $startInfo.ArgumentList.Add('--check-scale')
    $startInfo.ArgumentList.Add($Scale.ToString('R', [Globalization.CultureInfo]::InvariantCulture))
    if ($LatencySamples) {
        $startInfo.ArgumentList.Add('--check-latency')
        $startInfo.ArgumentList.Add($LatencySamples.ToString([Globalization.CultureInfo]::InvariantCulture))
    }
    $startInfo.UseShellExecute = $false
    $startInfo.RedirectStandardOutput = $true
    $startInfo.RedirectStandardError = $true
    $startInfo.StandardOutputEncoding = [Text.UTF8Encoding]::new($false)
    $startInfo.StandardErrorEncoding = [Text.UTF8Encoding]::new($false)
    $startInfo.Environment['WHOSTTY_CONFIG'] = Join-Path $checkDirectory 'config-must-not-be-created.toml'
    $nativeCheckProcess = [Diagnostics.Process]::new()
    $nativeCheckProcess.StartInfo = $startInfo
    if (-not $nativeCheckProcess.Start()) { throw 'Could not start the native check' }
    $stdoutTask = $nativeCheckProcess.StandardOutput.ReadToEndAsync()
    $stderrTask = $nativeCheckProcess.StandardError.ReadToEndAsync()
    $timeoutMilliseconds = if ($LatencySamples) { 660000 } else { 60000 }
    if (-not $nativeCheckProcess.WaitForExit($timeoutMilliseconds)) {
        $nativeCheckProcess.Kill($true)
        $nativeCheckProcess.WaitForExit()
        throw "Native runtime check exceeded $($timeoutMilliseconds / 1000) seconds"
    }
    $stdout = $stdoutTask.GetAwaiter().GetResult()
    $stderr = $stderrTask.GetAwaiter().GetResult()
    if ($nativeCheckProcess.ExitCode -ne 0) { throw "Native runtime check failed: $stderr $stdout" }
    $runtimeReport = $stdout.Trim() | ConvertFrom-Json
    if (-not $runtimeReport.ok -or -not $runtimeReport.pty -or -not $runtimeReport.input_roundtrip -or -not $runtimeReport.native_renderer -or -not $runtimeReport.text_shaping -or $runtimeReport.colored_pixels -lt 20) {
        throw "Native runtime check did not verify the PTY and rendered pixels: $stdout"
    }
    if ([Math]::Abs($runtimeReport.pixels_per_point - $Scale) -ge 0.01) {
        throw "Native runtime check used an unexpected rendering scale: $stdout"
    }
    if ($LatencySamples -and ($runtimeReport.latency.samples -ne $LatencySamples -or $null -eq $runtimeReport.latency.write_to_gpu_readback.p95)) {
        throw "Native runtime check did not collect the requested latency samples: $stdout"
    }
    if (Test-Path -LiteralPath $startInfo.Environment['WHOSTTY_CONFIG']) { throw 'Runtime check wrote user configuration' }
    if ($ReportPath) { [IO.File]::WriteAllText([IO.Path]::GetFullPath($ReportPath), $stdout.Trim(), [Text.UTF8Encoding]::new($false)) }
    Write-Output ($stdout.Trim())
} finally {
    if ($nativeCheckProcess) { $nativeCheckProcess.Dispose() }
    Remove-Item -LiteralPath $checkDirectory -Recurse -Force
}
