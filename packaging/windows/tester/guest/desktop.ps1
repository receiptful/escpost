Add-Type -AssemblyName UIAutomationClient
Add-Type -AssemblyName UIAutomationTypes
function Desktop-Windows {
    return [Windows.Automation.AutomationElement]::RootElement.FindAll([Windows.Automation.TreeScope]::Children, [Windows.Automation.Condition]::TrueCondition)
}
function Find-SetupWindow {
    foreach ($window in Desktop-Windows) {
        if ($window.Current.Name -match 'Setup.*ESCPost|ESCPost.*Setup') { return $window }
    }
    return $null
}
function Ui-Elements {
    param($Root)
    return $Root.FindAll([Windows.Automation.TreeScope]::Descendants, [Windows.Automation.Condition]::TrueCondition)
}
function Ui-Button {
    param($Root, [string]$Name)
    foreach ($element in Ui-Elements $Root) {
        if ($element.Current.ControlType -eq [Windows.Automation.ControlType]::Button -and $element.Current.Name.Replace('&', '') -match $Name -and $element.Current.IsEnabled) { return $element }
    }
    return $null
}
function Click-Button {
    param($Button)
    Assert-True ($null -ne $Button) 'Expected wizard button was not found.'
    $Button.GetCurrentPattern([Windows.Automation.InvokePattern]::Pattern).Invoke()
}
function Test-InstallerWizard {
    param([string]$Installer)
    $process = Start-Process $Installer -PassThru -ArgumentList @('/SP-', "/LOG=`"$evidence\wizard.log`"")
    try {
        Wait-Until { $null -ne (Find-SetupWindow) } 'Installer wizard did not appear.'
        $window = Find-SetupWindow
        $task = $null
        for ($step = 0; $step -lt 5; $step++) {
            $window = Find-SetupWindow
            foreach ($element in Ui-Elements $window) {
                if ($element.Current.Name -like 'Add ESCPost to my PATH*') { $task = $element; break }
            }
            if ($task) { break }
            Click-Button (Ui-Button $window '^Next')
            Start-Sleep -Milliseconds 500
        }
        (Ui-Elements $window | ForEach-Object { @{ name = $_.Current.Name; type = $_.Current.ControlType.ProgrammaticName; patterns = @($_.GetSupportedPatterns() | ForEach-Object ProgrammaticName) } }) | ConvertTo-Json -Depth 5 | Set-Content "$evidence\wizard-controls.json" -Encoding UTF8
        Assert-True ($null -ne $task) 'Default PATH task is missing from wizard.'
        Save-Screenshot 'installer-defaults'
        # Inno exposes this as a list item, without a .NET TogglePattern.
        # Leave the task untouched and prove its default through the installed
        # outcome instead of inferring checked state from list selection/focus.
        Click-Button (Ui-Button $window '^Next')
        Wait-Until { $w = Find-SetupWindow; $w -and (Ui-Button $w '^Install$') } 'Install button did not appear.'
        Click-Button (Ui-Button (Find-SetupWindow) '^Install$')
        Wait-Until { $w = Find-SetupWindow; $w -and (Ui-Button $w '^Finish$') } 'Visible installation did not finish.' 180
        Save-Screenshot 'installer-completed'
        Click-Button (Ui-Button (Find-SetupWindow) '^Finish$')
        Assert-True ($process.WaitForExit(10000)) 'Finished wizard did not exit.'
        Assert-True ($process.ExitCode -eq 0) 'Visible installation failed.'
        Assert-True (Test-Path $exe) 'Visible installer did not install ESCPost.'
        Assert-PathCount 1
    } finally {
        # Do not mask a failed wizard by calling its installation a success.
        if (-not $process.HasExited) { Stop-Process -Id $process.Id -Force -ErrorAction SilentlyContinue }
        $process.Dispose()
    }
}
function Test-WorkbenchShortcut {
    # Initialize Edge's normal profile without its first-run marketing dialog.
    # The shortcut must still navigate it to ESCPost itself.
    Start-Process "${env:ProgramFiles(x86)}\Microsoft\Edge\Application\msedge.exe" -ArgumentList @('--no-first-run', '--no-default-browser-check', 'about:blank')
    $shell = New-Object -ComObject WScript.Shell
    $link = $shell.CreateShortcut($shortcut)
    Assert-True ($link.TargetPath -ieq $exe -and $link.Arguments -eq 'serve --listen --web-listen') 'Wrong shortcut target/arguments.'
    $icon = Join-Path $installDir 'escpost-mark.ico'
    Assert-True ((Test-Path $icon) -and ($link.IconLocation -replace ',0$', '').Trim('"') -ieq $icon) 'Shortcut does not use the installed ESCPost icon.'
    $before = @(Get-Process escpost -ErrorAction SilentlyContinue | ForEach-Object Id)
    Start-Process $shortcut
    $script:shortcutProcess = $null
    try {
        Wait-Until {
            $matches = @(Get-Process escpost -ErrorAction SilentlyContinue | Where-Object { $_.Id -notin $before -and $_.Path -ieq $exe })
            if ($matches.Count -eq 1) { $script:shortcutProcess = $matches[0]; return $true }
            return $false
        } 'Shortcut did not launch installed ESCPost.'
        # Get-Process did not create this child. Retain its native handle while
        # it is alive so the exit code remains available after its console closes.
        $null = $script:shortcutProcess.Handle
        Wait-Until { try { (Invoke-WebRequest 'http://127.0.0.1:9000/health' -UseBasicParsing -TimeoutSec 2).StatusCode -eq 200 } catch { $false } } 'Shortcut server did not become healthy.'
        Wait-Until { @(Desktop-Windows | Where-Object { $_.Current.Name -match 'Workbench.*Edge|ESCPost.*Edge' }).Count -gt 0 } 'Shortcut did not open the workbench in Edge.' 45
        Save-Screenshot 'shortcut-browser'
        $console = $null
        foreach ($window in Desktop-Windows) {
            if ($window.Current.Name -like '*escpost*' -and $window.Current.Name -notlike '*Edge*') { $console = $window; break }
        }
        Assert-True ($null -ne $console) 'Shortcut console window was not found.'
        Assert-True ($shell.AppActivate($console.Current.ProcessId)) 'Could not focus the ESCPost console.'
        Start-Sleep -Milliseconds 300
        [Windows.Forms.SendKeys]::SendWait('^c')
        Assert-True ($script:shortcutProcess.WaitForExit(10000)) 'Ctrl+C did not stop ESCPost.'
        @{ pid = $script:shortcutProcess.Id; exitCode = $script:shortcutProcess.ExitCode } | ConvertTo-Json | Set-Content "$evidence\shortcut-exit.json" -Encoding UTF8
        Assert-True ($script:shortcutProcess.ExitCode -eq 0) "Ctrl+C exit code was $($script:shortcutProcess.ExitCode)."
    } finally {
        if ($script:shortcutProcess -and -not $script:shortcutProcess.HasExited) { Stop-Process -Id $script:shortcutProcess.Id -Force }
        if ($script:shortcutProcess) { $script:shortcutProcess.Dispose() }
    }
}
