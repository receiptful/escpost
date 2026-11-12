$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
$root = 'C:\Acceptance'
New-Item -ItemType Directory -Force $root | Out-Null
# Dockur creates Tester as the initial administrator. Provision a separate
# controller, then remove Tester from Administrators before sealing the baseline.
$random = New-Object byte[] 48
$generator = [Security.Cryptography.RandomNumberGenerator]::Create()
try { $generator.GetBytes($random) } finally { $generator.Dispose() }
$password = ConvertTo-SecureString ('Aa1!' + [Convert]::ToBase64String($random)) -AsPlainText -Force
if (-not (Get-LocalUser Controller -ErrorAction SilentlyContinue)) {
    New-LocalUser Controller -Password $password -PasswordNeverExpires | Out-Null
}
$administrators = Get-LocalGroup -SID 'S-1-5-32-544'
Add-LocalGroupMember -Group $administrators -Member Controller -ErrorAction SilentlyContinue
$users = Get-LocalGroup -SID 'S-1-5-32-545'
Add-LocalGroupMember -Group $users -Member Tester -ErrorAction SilentlyContinue
Set-LocalUser Tester -PasswordNeverExpires $true

$capability = Get-WindowsCapability -Online | Where-Object Name -Like 'OpenSSH.Server*'
if ($capability.State -ne 'Installed') { Add-WindowsCapability -Online -Name $capability.Name | Out-Null }
$sshRoot = 'C:\ProgramData\ssh'
New-Item -ItemType Directory -Force $sshRoot | Out-Null
$keys = Join-Path $sshRoot 'administrators_authorized_keys'
Copy-Item C:\OEM\controller_key.pub $keys -Force
& icacls.exe $keys /inheritance:r /grant '*S-1-5-18:F' '*S-1-5-32-544:F' | Out-Null
if ($LASTEXITCODE) { throw 'Could not secure SSH key.' }
$config = Join-Path $sshRoot 'sshd_config'
if (-not (Test-Path $config)) { Copy-Item "$env:WINDIR\System32\OpenSSH\sshd_config_default" $config }
$contents = Get-Content $config -Raw
# Windows OpenSSH requires lowercase names in its access-control patterns.
Set-Content $config ("PasswordAuthentication no`r`nAllowUsers controller`r`n" + $contents) -Encoding Ascii
Set-Service sshd -StartupType Automatic
Restart-Service sshd
if (-not (Get-NetFirewallRule -Name 'ESCPostTester-SSH' -ErrorAction SilentlyContinue)) {
    New-NetFirewallRule -Name 'ESCPostTester-SSH' -DisplayName 'ESCPost tester control' -Enabled True -Direction Inbound -Protocol TCP -Action Allow -LocalPort 22 | Out-Null
}

Copy-Item C:\OEM\agent.ps1 "$root\agent.ps1" -Force
Copy-Item C:\OEM\ready.ps1 "$root\ready.ps1" -Force
New-Item -ItemType Directory -Force "$root\jobs" | Out-Null
& icacls.exe $root /grant 'Tester:(OI)(CI)M' | Out-Null
if ($LASTEXITCODE) { throw 'Could not prepare acceptance workspace.' }
& C:\OEM\tasks.ps1
Remove-LocalGroupMember -Group $administrators -Member Tester
@{ created = [DateTime]::UtcNow.ToString('o'); os = (Get-CimInstance Win32_OperatingSystem).Version } | ConvertTo-Json | Set-Content "$root\provisioned.json" -Encoding UTF8
