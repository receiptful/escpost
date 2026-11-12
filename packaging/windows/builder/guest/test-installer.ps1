param(
    [Parameter(Mandatory = $true)][string]$Installer,
    [Parameter(Mandatory = $true)][string]$Directory
)
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest

function Run-Setup {
    param([string]$File, [string[]]$Options)
    $process = Start-Process -FilePath $File -Wait -PassThru -ArgumentList (@('/VERYSILENT', '/SUPPRESSMSGBOXES', '/NORESTART', '/SP-') + $Options)
    if ($process.ExitCode -ne 0) { throw "Installer process exited with $($process.ExitCode)" }
}
function Read-UserPath {
    $value = $environmentKey.GetValue('Path', '', [Microsoft.Win32.RegistryValueOptions]::DoNotExpandEnvironmentNames)
    return [string]$value
}
function Assert-PathCount {
    param([int]$Expected)
    $entries = @((Read-UserPath).Split(';') | Where-Object { $_.Trim().Trim('"').TrimEnd('\') -ieq $installDir })
    if ($entries.Count -ne $Expected) { throw "Expected $Expected ESCPost PATH entries, got $($entries.Count)." }
}

$installDir = Join-Path $Directory 'Programs With Spaces\ESCPost'
$shortcut = Join-Path ([Environment]::GetFolderPath('Programs')) 'ESCPost Workbench.lnk'
if ((Test-Path $installDir) -or (Test-Path $shortcut) -or (Test-Path 'HKCU:\Software\Receiptful\ESCPost')) {
    throw 'Installer tests require no existing ESCPost installation for the builder user.'
}
New-Item -ItemType Directory -Force -Path $Directory | Out-Null
$environmentKey = [Microsoft.Win32.Registry]::CurrentUser.CreateSubKey('Environment')
$hadPath = $environmentKey.GetValueNames() -contains 'Path'
$originalPath = Read-UserPath
$originalKind = [Microsoft.Win32.RegistryValueKind]::ExpandString
if ($hadPath) { $originalKind = $environmentKey.GetValueKind('Path') }
$baseline = '%SystemRoot%\System32;C:\EscpostBuilder\Unrelated;'
$configDir = Join-Path $env:APPDATA 'receiptful\escpost\config'
New-Item -ItemType Directory -Force -Path $configDir | Out-Null
$sentinel = Join-Path $configDir "installer-test-$([Guid]::NewGuid()).txt"
Set-Content $sentinel 'preserve user data'
$uninstaller = Join-Path $installDir 'unins000.exe'
try {
    $environmentKey.SetValue('Path', $baseline, [Microsoft.Win32.RegistryValueKind]::ExpandString)
    $options = @("/DIR=`"$installDir`"", "/LOG=`"$Directory\install.log`"")
    Run-Setup $Installer $options
    Assert-PathCount 1
    & (Join-Path $installDir 'escpost.exe') --version
    if ($LASTEXITCODE -ne 0) { throw 'Installed executable failed.' }
    $shell = New-Object -ComObject WScript.Shell
    $link = $shell.CreateShortcut($shortcut)
    if ($link.Arguments -ne 'serve --listen --web-listen' -or $link.TargetPath -ne (Join-Path $installDir 'escpost.exe')) {
        throw 'Workbench shortcut does not launch the installed executable correctly.'
    }
    $icon = Join-Path $installDir 'escpost-mark.ico'
    if (-not (Test-Path $icon) -or ($link.IconLocation -replace ',0$', '').Trim('"') -ine $icon) {
        throw 'Workbench shortcut does not use the installed ESCPost icon.'
    }
    # Reinstallation follows the same AppId and must not duplicate PATH.
    Run-Setup $Installer $options
    Assert-PathCount 1
    Run-Setup $Installer ($options + '/TASKS=""')
    Assert-PathCount 0
    if ((Read-UserPath) -ne $baseline) { throw 'Opting out during an upgrade changed unrelated PATH entries.' }
    Run-Setup $Installer ($options + '/TASKS=addtopath')
    Assert-PathCount 1
    Run-Setup $uninstaller @("/LOG=`"$Directory\uninstall.log`"")
    if ((Read-UserPath) -ne $baseline) { throw 'Uninstall changed unrelated PATH entries.' }
    if (Test-Path $shortcut) { throw 'Uninstall left the Start-menu shortcut.' }
    if (Test-Path $icon) { throw 'Uninstall left the ESCPost icon.' }
    if (-not (Test-Path $sentinel)) { throw 'Uninstall removed user configuration.' }

    # Respect a pre-existing, quoted entry; never claim it for uninstall.
    $existing = "$baseline;`"$installDir\`""
    $environmentKey.SetValue('Path', $existing, [Microsoft.Win32.RegistryValueKind]::ExpandString)
    Run-Setup $Installer $options
    Assert-PathCount 1
    Run-Setup $uninstaller @()
    if ((Read-UserPath) -ne $existing) { throw 'Uninstall removed a pre-existing PATH entry.' }

    $environmentKey.SetValue('Path', $baseline, [Microsoft.Win32.RegistryValueKind]::ExpandString)
    Run-Setup $Installer ($options + '/TASKS=""')
    Assert-PathCount 0
    Run-Setup $Installer $options
    Assert-PathCount 0
    Run-Setup $uninstaller @()
    if ((Read-UserPath) -ne $baseline) { throw 'PATH opt-out was not preserved.' }
    Write-Host 'Installer checks passed (install, reinstall, PATH ownership/opt-out, shortcut, uninstall, data preservation).'
}
finally {
    try {
        if (Test-Path $uninstaller) { Run-Setup $uninstaller @() }
    }
    finally {
        if ($hadPath) { $environmentKey.SetValue('Path', $originalPath, $originalKind) }
        else { $environmentKey.DeleteValue('Path', $false) }
        $environmentKey.Dispose()
        Remove-Item -LiteralPath $sentinel -Force -ErrorAction SilentlyContinue
    }
}
