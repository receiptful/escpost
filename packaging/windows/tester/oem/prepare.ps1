# Refresh the harness before sealing a new baseline. SSH can become available
# a few seconds before first-boot setup finishes.
$ErrorActionPreference = 'Stop'
# Dockur's RTC uses the Linux container's local clock (UTC). Match it so a
# reboot does not interpret UTC as Pacific time before Windows time sync runs.
Set-TimeZone -Id 'UTC'
$deadline = [DateTime]::UtcNow.AddMinutes(5)
while (-not (Test-Path C:\Acceptance\provisioned.json)) {
    if ([DateTime]::UtcNow -gt $deadline) { throw 'First-boot setup did not finish; inspect C:\oem-bootstrap.log.' }
    Start-Sleep -Seconds 2
}
Copy-Item "$PSScriptRoot\agent.ps1", "$PSScriptRoot\ready.ps1" C:\Acceptance -Force
& "$PSScriptRoot\tasks.ps1"
# Keep the controller key-only, with no reusable password in staged OEM files.
$random = New-Object byte[] 48
$generator = [Security.Cryptography.RandomNumberGenerator]::Create()
try { $generator.GetBytes($random) } finally { $generator.Dispose() }
$password = ConvertTo-SecureString ('Aa1!' + [Convert]::ToBase64String($random)) -AsPlainText -Force
Set-LocalUser Controller -Password $password
Remove-Item C:\OEM\controller-password.txt -Force -ErrorAction SilentlyContinue
$administratorMembers = @(Get-LocalGroupMember -SID 'S-1-5-32-544')
$tester = Get-LocalUser Tester
if ($administratorMembers.SID.Value -contains $tester.SID.Value) { throw 'Tester must not belong to Administrators.' }
