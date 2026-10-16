#requires -Version 7.0
param(
    [Parameter(Mandatory = $true)][string]$Executable,
    [string]$ReportPath
)

$ErrorActionPreference = 'Stop'
$checkDirectory = Join-Path ([IO.Path]::GetTempPath()) ('whostty-check-' + [guid]::NewGuid().ToString('N'))
$null = New-Item -ItemType Directory -Path $checkDirectory
$nativeCheckProcess = $null
try {
    $startInfo = [Diagnostics.ProcessStartInfo]::new()
    $startInfo.FileName = (Resolve-Path -LiteralPath $Executable).ProviderPath
    $startInfo.ArgumentList.Add('--check')
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
    if (-not $nativeCheckProcess.WaitForExit(60000)) {
        $nativeCheckProcess.Kill($true)
        $nativeCheckProcess.WaitForExit()
        throw 'Native runtime check exceeded 60 seconds'
    }
    $stdout = $stdoutTask.GetAwaiter().GetResult()
    $stderr = $stderrTask.GetAwaiter().GetResult()
    if ($nativeCheckProcess.ExitCode -ne 0) { throw "Native runtime check failed: $stderr $stdout" }
    $runtimeReport = $stdout.Trim() | ConvertFrom-Json
    if (-not $runtimeReport.ok -or -not $runtimeReport.pty -or -not $runtimeReport.native_renderer -or -not $runtimeReport.text_shaping -or $runtimeReport.colored_pixels -lt 20) {
        throw "Native runtime check did not verify the PTY and rendered pixels: $stdout"
    }
    if (Test-Path -LiteralPath $startInfo.Environment['WHOSTTY_CONFIG']) { throw 'Runtime check wrote user configuration' }
    if ($ReportPath) { [IO.File]::WriteAllText([IO.Path]::GetFullPath($ReportPath), $stdout.Trim(), [Text.UTF8Encoding]::new($false)) }
    Write-Output ($stdout.Trim())
} finally {
    if ($nativeCheckProcess) { $nativeCheckProcess.Dispose() }
    Remove-Item -LiteralPath $checkDirectory -Recurse -Force
}
