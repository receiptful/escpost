param([switch]$RunTests)
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
$root = 'C:\Acceptance'
$principal = New-Object Security.Principal.WindowsPrincipal([Security.Principal.WindowsIdentity]::GetCurrent())
@{
    user = [Security.Principal.WindowsIdentity]::GetCurrent().Name
    administrator = $principal.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)
    session = [Diagnostics.Process]::GetCurrentProcess().SessionId
    boot = (Get-CimInstance Win32_OperatingSystem).LastBootUpTime.ToUniversalTime().ToString('o')
} | ConvertTo-Json | Set-Content "$root\desktop-ready.json" -Encoding UTF8
if ($RunTests -and (Test-Path "$root\active-job.txt")) {
    $job = (Get-Content "$root\active-job.txt" -Raw).Trim()
    if ($job -notmatch '^[a-zA-Z0-9_-]+$') { throw 'Invalid acceptance job.' }
    try {
        & "$root\jobs\$job\verify.ps1" -JobDirectory "$root\jobs\$job"
    }
    catch {
        @{ status = 'failed'; error = $_.Exception.Message } | ConvertTo-Json | Set-Content "$root\jobs\$job\evidence\results.json" -Encoding UTF8
    }
}
