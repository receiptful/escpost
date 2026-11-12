$ErrorActionPreference = 'Stop'
$r = Get-Content C:\Acceptance\desktop-ready.json -Raw | ConvertFrom-Json
$boot = (Get-CimInstance Win32_OperatingSystem).LastBootUpTime.ToUniversalTime().ToString('o')
if ($r.administrator -or $r.session -eq 0 -or $r.boot -ne $boot) { exit 1 }
