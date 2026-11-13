param(
    [Parameter(Mandatory = $true)][ValidatePattern('^[0-9a-fA-F]{4}$')][string]$VendorId,
    [Parameter(Mandatory = $true)][ValidatePattern('^[0-9a-fA-F]{4}$')][string]$ProductId,
    [switch]$WithoutSpooler,
    [ValidatePattern('^[a-zA-Z0-9_-]+$')][string]$Run = 'normal'
)
$ErrorActionPreference = 'Stop'
$root = 'C:\Users\Tester\Desktop\USBPRINT-Production'
$phase = if ($WithoutSpooler) { 'spooler-off' } else { 'spooler-on' }
$job = "$VendorId-$ProductId-$phase-$Run"
$output = Join-Path $root $job
$result = Join-Path $output 'results.json'
if (Test-Path $output) { throw "Evidence already exists: $output" }
$original = (Get-Service Spooler).Status
$name = 'ESCPost USBPRINT probe'
try {
    if ($WithoutSpooler) { Stop-Service Spooler -Force }
    if (-not $WithoutSpooler -and $original -ne 'Running') { throw 'Spooler was not running for the on test.' }
    $arguments = '-NoLogo -NoProfile -ExecutionPolicy Bypass -File "' + $root + '\usbprint.ps1" -Executable "' + $root + '\escpost.exe" -VendorId ' + $VendorId + ' -ProductId ' + $ProductId + ' -OutputDirectory "' + $output + '" -Print'
    $action = New-ScheduledTaskAction -Execute 'powershell.exe' -Argument $arguments
    $principal = New-ScheduledTaskPrincipal -UserId Tester -LogonType Interactive -RunLevel Limited
    $settings = New-ScheduledTaskSettingsSet -ExecutionTimeLimit (New-TimeSpan -Minutes 5)
    Register-ScheduledTask -TaskName $name -Action $action -Principal $principal -Settings $settings -Force | Out-Null
    Start-ScheduledTask -TaskName $name
    $deadline = [DateTime]::UtcNow.AddSeconds(240)
    while (-not (Test-Path $result)) {
        if ([DateTime]::UtcNow -gt $deadline) { throw 'Probe timed out; inspect the scheduled task and desktop.' }
        Start-Sleep -Seconds 2
    }
    Get-Content $result
    $report = Get-Content $result -Raw | ConvertFrom-Json
    if ($report.status -ne 'passed') { throw 'Probe reported failure.' }
    if ($WithoutSpooler -and $report.spooler -ne 'Stopped') { throw 'Spooler was not stopped.' }
} finally {
    Stop-ScheduledTask -TaskName $name -ErrorAction SilentlyContinue
    Unregister-ScheduledTask -TaskName $name -Confirm:$false -ErrorAction SilentlyContinue
    if ($original -eq 'Running' -and (Get-Service Spooler).Status -ne 'Running') { Start-Service Spooler }
}
