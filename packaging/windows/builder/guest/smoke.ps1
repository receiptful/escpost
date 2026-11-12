param(
    [Parameter(Mandatory = $true)][string]$Executable,
    [Parameter(Mandatory = $true)][string]$Directory
)
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
New-Item -ItemType Directory -Force -Path $Directory | Out-Null
$Executable = (Resolve-Path $Executable).Path

Push-Location $Directory
try {
    & $Executable --version
    if ($LASTEXITCODE -ne 0) { throw 'Release executable failed to start.' }
    '1b 40 45 53 43 50 6f 73 74 0a 0a 0a' | Set-Content -Encoding Ascii smoke.hex
    & $Executable render smoke.hex --profile REFERENCE --output-dir rendered
    if ($LASTEXITCODE -ne 0) { throw 'Release render failed.' }
    $png = @(Get-ChildItem rendered -Filter '*.png')
    if ($png.Count -ne 1) { throw 'Expected one rendered PNG.' }
    $bytes = [IO.File]::ReadAllBytes($png[0].FullName)
    if ([BitConverter]::ToString($bytes[0..7]) -ne '89-50-4E-47-0D-0A-1A-0A') { throw 'Invalid PNG output.' }

    $stderr = Join-Path $Directory 'server-stderr.txt'
    $process = Start-Process -FilePath $Executable -PassThru -WorkingDirectory $Directory `
        -ArgumentList @('serve', '--listen', '127.0.0.1:0', '--web-listen', '127.0.0.1:0', '--non-interactive', '--no-open') `
        -RedirectStandardOutput (Join-Path $Directory 'server-stdout.txt') -RedirectStandardError $stderr
    try {
        $address = $null
        for ($attempt = 0; $attempt -lt 120; $attempt++) {
            if ($process.HasExited) { throw "Server exited: $(Get-Content $stderr -Raw)" }
            $log = Get-Content $stderr -Raw -ErrorAction SilentlyContinue
            if ($log -match 'http://127\.0\.0\.1:(\d+)') { $address = "http://127.0.0.1:$($Matches[1])"; break }
            Start-Sleep -Milliseconds 250
        }
        if ($null -eq $address) { throw 'Timed out waiting for the release web server.' }
        $health = Invoke-WebRequest "$address/health" -UseBasicParsing -TimeoutSec 10
        if ($health.StatusCode -ne 200) { throw 'Health endpoint failed.' }
        $page = Invoke-WebRequest "$address/" -UseBasicParsing -TimeoutSec 10
        if ($page.Content -notmatch '(?:src|href)="(/assets/[^" ]+\.js)"') { throw 'Embedded frontend script is missing.' }
        $asset = Invoke-WebRequest "$address$($Matches[1])" -UseBasicParsing -TimeoutSec 10
        if ($asset.StatusCode -ne 200 -or $asset.RawContentLength -eq 0) { throw 'Embedded frontend asset failed.' }
    }
    finally {
        if (-not $process.HasExited) { Stop-Process -Id $process.Id -Force }
        $process.WaitForExit()
        $process.Dispose()
    }
    Write-Host 'Release smoke checks passed (version, rendering, HTTP, embedded assets).'
}
finally { Pop-Location }
