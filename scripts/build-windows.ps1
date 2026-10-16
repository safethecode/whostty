[CmdletBinding()]
param()

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
$ProjectRoot = Split-Path $PSScriptRoot -Parent
Push-Location $ProjectRoot
try {
    if (-not $IsWindows) { throw 'Run this script in PowerShell 7 on Windows.' }
    $Target = 'x86_64-pc-windows-msvc'
    & cargo build --release --locked --package whostty --target $Target
    if ($LASTEXITCODE -ne 0) { throw 'Release build failed.' }

    $PackagePath = Join-Path $ProjectRoot 'dist/whostty-windows-x64'
    New-Item -ItemType Directory -Force -Path $PackagePath | Out-Null
    Copy-Item "target/$Target/release/whostty.exe" $PackagePath -Force
    Copy-Item 'context/LICENSE' $PackagePath -Force
    & cargo about generate --locked -o "$PackagePath/THIRD_PARTY_LICENSES.html" context/third-party-licenses.hbs
    if ($LASTEXITCODE -ne 0) { throw 'License notice generation failed. Install cargo-about 0.9.2 with the cli feature.' }
    $Archive = Join-Path $ProjectRoot 'dist/whostty-windows-x64.zip'
    Compress-Archive -Path "$PackagePath/*" -DestinationPath $Archive -Force
    $Hash = (Get-FileHash -Algorithm SHA256 $Archive).Hash.ToLowerInvariant()
    "$Hash  whostty-windows-x64.zip" | Set-Content -Encoding ascii "$Archive.sha256"
    Write-Host "Package: $Archive"
} finally { Pop-Location }
