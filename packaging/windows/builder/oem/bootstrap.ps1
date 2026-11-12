$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest

$capability = Get-WindowsCapability -Online | Where-Object Name -Like 'OpenSSH.Server*'
if ($null -eq $capability) { throw 'Windows OpenSSH Server is unavailable.' }
if ($capability.State -ne 'Installed') {
    Add-WindowsCapability -Online -Name $capability.Name | Out-Null
}

$sshRoot = 'C:\ProgramData\ssh'
$authorizedKeys = Join-Path $sshRoot 'administrators_authorized_keys'
New-Item -ItemType Directory -Force -Path $sshRoot | Out-Null
Copy-Item -LiteralPath 'C:\OEM\builder_key.pub' -Destination $authorizedKeys -Force
& icacls.exe $authorizedKeys /inheritance:r /grant '*S-1-5-18:F' '*S-1-5-32-544:F' | Out-Null
if ($LASTEXITCODE -ne 0) { throw 'Could not secure the SSH authorized keys.' }

# Only the generated local key can authenticate. The host publishes SSH on loopback.
$config = Join-Path $sshRoot 'sshd_config'
if (-not (Test-Path $config)) {
    Copy-Item "$env:WINDIR\System32\OpenSSH\sshd_config_default" $config
}
$contents = Get-Content $config -Raw
$contents = "PasswordAuthentication no`r`nPubkeyAuthentication yes`r`n" + $contents
Set-Content -LiteralPath $config -Value $contents -Encoding Ascii
Set-Service sshd -StartupType Automatic
Restart-Service sshd
if (-not (Get-NetFirewallRule -Name 'ESCPostBuilder-SSH' -ErrorAction SilentlyContinue)) {
    New-NetFirewallRule -Name 'ESCPostBuilder-SSH' -DisplayName 'ESCPost builder SSH' `
        -Enabled True -Direction Inbound -Protocol TCP -Action Allow -LocalPort 22 | Out-Null
}
