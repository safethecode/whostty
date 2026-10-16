$ErrorActionPreference = 'Stop'
$Escape = [char]27
Write-Host 'Whostty Windows manual smoke test'
Write-Host ('Window: {0} columns x {1} rows' -f [Console]::WindowWidth, [Console]::WindowHeight)
Write-Host "$Escape[31mRed $Escape[32mGreen $Escape[34mBlue$Escape[0m"
Write-Host "$Escape[38;2;151;219;174mTruecolor green$Escape[0m"
Write-Host "$Escape[1mBold$Escape[0m  $Escape[4mUnderline$Escape[0m"
Write-Host "Korean: 한글 입력 확인 / Wide: 가나다라마바사 / Combining: e$([char]0x301)"
1..100 | ForEach-Object { Write-Host "Scrollback line $_" }
Read-Host 'Scroll up and down, resize the window, then press Enter'
try {
    [Console]::Write("$Escape[?1049h$Escape[2J$Escape[H")
    Write-Host 'Alternate screen. Resize the window and type Korean below.'
    $Value = Read-Host 'Input'
    Write-Host "Received: $Value"
    Read-Host 'Press Enter to restore the normal screen'
} finally { [Console]::Write("$Escape[?1049l$Escape[0m") }
Write-Host 'Normal screen restored. Test Ctrl+C with: Start-Sleep 60'
