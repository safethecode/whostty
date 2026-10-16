# Loaded inside the child PowerShell, after its existing profile and prompt.
# No profile/config files are modified, and no execution policy is changed.
& {
    $originalPrompt = $function:prompt
    if ($null -eq $originalPrompt) { return }
    $function:global:prompt = {
        # Invoke the existing prompt first, before any command changes its view of $?.
        $promptOutput = & $originalPrompt
        $lastExitVariable = Get-Variable -Name LASTEXITCODE -Scope Global -ErrorAction Ignore
        try {
            if ($pwd.Provider.Name -eq 'FileSystem') {
                $uri = [Uri]::new($pwd.ProviderPath + [IO.Path]::DirectorySeparatorChar)
                if ($uri.IsAbsoluteUri -and $uri.IsFile) {
                    [Console]::Write(([string][char]27) + ']7;' + $uri.AbsoluteUri + [char]7)
                }
            }
        } catch {
            # Directory reporting must not prevent the user's prompt from appearing.
        } finally {
            if ($null -ne $lastExitVariable) { $global:LASTEXITCODE = $lastExitVariable.Value }
        }
        $promptOutput
    }.GetNewClosure()
}
