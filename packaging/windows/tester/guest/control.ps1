param(
    [Parameter(Mandatory = $true)][ValidatePattern('^[a-zA-Z0-9_-]+$')][string]$Job,
    [Parameter(Mandatory = $true)][ValidateSet('Stage', 'Start', 'Stop', 'Status')][string]$Action
)
$ErrorActionPreference = 'Stop'
$root = 'C:\Acceptance'
$directory = Join-Path "$root\jobs" $Job
switch ($Action) {
    Stage {
        if (Test-Path $directory) { throw 'Acceptance job already exists.' }
        New-Item -ItemType Directory -Path $directory | Out-Null
        Copy-Item "$env:USERPROFILE\incoming\$Job\*" $directory
        Copy-Item "$directory\agent.ps1" "$root\agent.ps1" -Force
        New-Item -ItemType Directory -Path "$directory\evidence" | Out-Null
        $request = Get-Content "$directory\request.json" -Raw | ConvertFrom-Json
        if ((Get-FileHash "$directory\candidate.exe" -Algorithm SHA256).Hash -ne $request.candidateSha256) { throw 'Candidate hash mismatch.' }
        if ($request.previousSha256 -and (Get-FileHash "$directory\previous.exe" -Algorithm SHA256).Hash -ne $request.previousSha256) { throw 'Previous hash mismatch.' }
        Set-Content "$root\active-job.txt" $Job -Encoding Ascii
    }
    Start { Start-ScheduledTask -TaskName 'ESCPost acceptance' }
    Stop { Stop-ScheduledTask -TaskName 'ESCPost acceptance' }
    Status {
        $path = "$directory\evidence\results.json"
        if (Test-Path $path) {
            try { (Get-Content $path -Raw | ConvertFrom-Json).status }
            catch { 'running' }
        } else { 'running' }
    }
}
