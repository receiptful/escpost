$ErrorActionPreference = 'Stop'
$principal = New-ScheduledTaskPrincipal -UserId Tester -LogonType Interactive -RunLevel Limited
$settings = New-ScheduledTaskSettingsSet -ExecutionTimeLimit (New-TimeSpan -Minutes 30) -MultipleInstances IgnoreNew
$command = '-NoLogo -NoProfile -ExecutionPolicy Bypass -File C:\Acceptance\agent.ps1'
$ready = New-ScheduledTaskAction -Execute 'powershell.exe' -Argument $command
$trigger = New-ScheduledTaskTrigger -AtLogOn -User Tester
Register-ScheduledTask -TaskName 'ESCPost desktop' -Action $ready -Principal $principal -Trigger $trigger -Settings $settings -Force | Out-Null
# Tests only start on an explicit host request, including after the PATH reboot.
# A later manual boot must never resume an interrupted acceptance run.
$tests = New-ScheduledTaskAction -Execute 'powershell.exe' -Argument "$command -RunTests"
Register-ScheduledTask -TaskName 'ESCPost acceptance' -Action $tests -Principal $principal -Settings $settings -Force | Out-Null
