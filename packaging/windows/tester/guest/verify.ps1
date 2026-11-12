param([Parameter(Mandatory = $true)][string]$JobDirectory)
. (Join-Path $PSScriptRoot 'common.ps1')
$evidence = Join-Path $JobDirectory 'evidence'
$request = Get-Content "$JobDirectory\request.json" -Raw | ConvertFrom-Json
$phaseFile = Join-Path $JobDirectory 'phase.json'
$installDir = Join-Path $env:LOCALAPPDATA 'Programs\ESCPost'
$exe = Join-Path $installDir 'escpost.exe'
$installer = Join-Path $JobDirectory 'candidate.exe'
$uninstaller = Join-Path $installDir 'unins000.exe'
$shortcut = Join-Path ([Environment]::GetFolderPath('Programs')) 'ESCPost Workbench.lnk'
$config = Join-Path $env:APPDATA 'receiptful\escpost\config'
$sentinel = Join-Path $config 'acceptance-preserve.txt'
$boot = (Get-CimInstance Win32_OperatingSystem).LastBootUpTime.ToUniversalTime().ToString('o')
$env:DO_NOT_TRACK = '1'
$env:ESCPOST_TELEMETRY = '0'
Remove-Item Env:ESCPOST_CONFIG_DIR -ErrorAction SilentlyContinue
$phase = $null
if (Test-Path $phaseFile) { $phase = Get-Content $phaseFile -Raw | ConvertFrom-Json }
if ($phase -and $phase.boot -eq $boot) { return }
if (Test-Path "$evidence\results.json") {
    $loaded = Get-Content "$evidence\results.json" -Raw | ConvertFrom-Json
    if ($loaded.status -in @('passed', 'failed')) { return }
    $result = @{ status = 'running'; updated = ''; checks = @($loaded.checks); metadata = $loaded.metadata }
} else {
    $result = @{
        status = 'running'; updated = ''; checks = @()
        metadata = @{
            candidateSha256 = $request.candidateSha256; previousSha256 = $request.previousSha256
            requestedAt = $request.requestedAt
            os = (Get-CimInstance Win32_OperatingSystem | Select-Object Caption, Version, BuildNumber)
            user = [Security.Principal.WindowsIdentity]::GetCurrent().Name
        }
    }
}
Start-Transcript -Path "$evidence\transcript-$([DateTime]::UtcNow.ToString('yyyyMMddTHHmmss')).log" | Out-Null
try {
    Check 'standard-user desktop token' {
        $identity = [Security.Principal.WindowsIdentity]::GetCurrent()
        $principal = New-Object Security.Principal.WindowsPrincipal($identity)
        Assert-True (-not $principal.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)) 'Acceptance must not run as administrator.'
        Assert-True ($env:USERNAME -eq 'Tester') 'Wrong acceptance account.'
        Assert-True ([Diagnostics.Process]::GetCurrentProcess().SessionId -ne 0) 'No interactive desktop session.'
    }
    Check 'installer hashes' {
        Assert-True ((Get-FileHash $installer -Algorithm SHA256).Hash -eq $request.candidateSha256) 'Candidate hash mismatch.'
        if ($request.previousSha256) { Assert-True ((Get-FileHash "$JobDirectory\previous.exe" -Algorithm SHA256).Hash -eq $request.previousSha256) 'Previous hash mismatch.' }
    }
    if (-not $phase) {
        Check 'clean application environment' {
            Assert-True (-not (Test-Path $exe)) 'ESCPost is already installed.'
            Assert-True (-not (Get-Command escpost.exe -ErrorAction SilentlyContinue)) 'ESCPost is already on PATH.'
            foreach ($tool in @('cargo.exe', 'rustc.exe', 'cl.exe', 'ISCC.exe', 'node.exe', 'bun.exe')) {
                Assert-True (-not (Get-Command $tool -ErrorAction SilentlyContinue)) "Unexpected development tool: $tool"
            }
            # Windows App Execution Aliases may advertise Python even with no
            # interpreter installed. Inspect installed products and common roots.
            $products = @(Get-ItemProperty 'HKLM:\Software\Microsoft\Windows\CurrentVersion\Uninstall\*', 'HKLM:\Software\WOW6432Node\Microsoft\Windows\CurrentVersion\Uninstall\*', 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\*' -ErrorAction SilentlyContinue)
            $unexpected = @($products | Where-Object { $_.PSObject.Properties['DisplayName'] -and $_.DisplayName -match 'Python|Visual Studio|Inno Setup|Rustup|Node.js' })
            Assert-True ($unexpected.Count -eq 0) 'Development runtimes are installed.'
            Assert-True (-not (Test-Path 'C:\EscpostBuilder')) 'Builder files leaked into tester.'
        }
        $originalPath = User-Path
        New-Item -ItemType Directory -Force $config | Out-Null
        Set-Content $sentinel 'acceptance data must survive'
        if ($request.previousSha256) {
            Check 'previous-version installation' { Run-Setup "$JobDirectory\previous.exe" 'previous-install'; Assert-True (Test-Path $exe) 'Previous installer did not install ESCPost.' }
            $previousExeHash = (Get-FileHash $exe -Algorithm SHA256).Hash
        } else { $previousExeHash = '' }
        . "$PSScriptRoot\desktop.ps1"
        Check 'installer wizard default PATH selection' { Test-InstallerWizard $installer }
        Check 'default per-user installation' {
            Assert-True (Test-Path $exe) 'Installed executable missing.'
            Assert-True (Test-Path $uninstaller) 'Uninstaller missing.'
            Assert-True (Test-Path $shortcut) 'Start-menu shortcut missing.'
            $registration = Get-ItemProperty 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\{8F907A59-EC48-4FD0-9E1D-31B920D195F4}_is1'
            Assert-True ($registration.InstallLocation.TrimEnd('\') -ieq $installDir) 'Wrong per-user installation directory.'
            Assert-PathCount 1
            Run-Cli @('--version')
        }
        if ($request.previousSha256) {
            Check 'upgrade preserves configuration and replaces executable' {
                Assert-True ((Get-Content $sentinel -Raw).Trim() -eq 'acceptance data must survive') 'Upgrade changed user data.'
                Assert-True ((Get-FileHash $exe -Algorithm SHA256).Hash -ne $previousExeHash) 'Upgrade did not replace the executable; use distinct application versions.'
            }
        } else { $result.checks += @{ name = 'previous-version upgrade'; status = 'not-run'; reason = 'No --previous installer supplied.' } }
        @{ boot = $boot; originalPath = $originalPath } | ConvertTo-Json | Set-Content $phaseFile -Encoding UTF8
        $result.status = 'awaiting-reboot'
        Save-Results
        return
    }
    Check 'PATH from a fresh Windows logon' {
        $command = Get-Command escpost.exe -ErrorAction Stop
        Assert-True ($command.Source -ieq $exe) "PATH resolves to $($command.Source)."
        Assert-PathCount 1
        $version = & escpost --version
        Assert-True ($LASTEXITCODE -eq 0 -and $version -match '^escpost \d+\.\d+\.\d+') 'Command failed via fresh PATH.'
    }
    . "$PSScriptRoot\runtime.ps1"
    Test-InstalledRuntime
    . "$PSScriptRoot\desktop.ps1"
    Check 'Start-menu shortcut and Ctrl+C shutdown' { Test-WorkbenchShortcut }
    Check 'reinstall, PATH opt-out, ownership, and uninstall' {
        Run-Setup $installer 'reinstall'
        Assert-PathCount 1
        Run-Setup $installer 'opt-out' @('/TASKS=""')
        Assert-PathCount 0
        Assert-True ((User-Path) -ceq $phase.originalPath) 'Opt-out changed unrelated PATH entries.'
        Run-Setup $installer 'remember-opt-out'
        Assert-PathCount 0
        Run-Setup $installer 'opt-in' @('/TASKS=addtopath')
        Assert-PathCount 1
        Run-Setup $uninstaller 'uninstall'
        Assert-True ((User-Path) -ceq $phase.originalPath) 'Uninstall changed unrelated PATH entries.'
        Assert-True (-not (Test-Path $exe)) 'Uninstall left executable.'
        Assert-True (-not (Test-Path $shortcut)) 'Uninstall left shortcut.'
        Assert-True (-not (Test-Path 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\{8F907A59-EC48-4FD0-9E1D-31B920D195F4}_is1')) 'Uninstall registration remains.'
        Assert-True ((Get-Content $sentinel -Raw).Trim() -eq 'acceptance data must survive') 'Uninstall changed user data.'
        $existing = "$($phase.originalPath);`"$installDir\`""
        Set-UserPath $existing
        Run-Setup $installer 'pre-existing-path'
        Assert-PathCount 1
        Run-Setup $uninstaller 'pre-existing-path-uninstall'
        Assert-True ((User-Path) -ceq $existing) 'Uninstall removed a pre-existing PATH entry.'
        Set-UserPath $phase.originalPath
    }
    $result.checks += @{ name = 'browser-download reputation'; status = 'not-run'; reason = 'Installer transferred locally; no public download URL tested.' }
    $result.checks += @{ name = 'physical USB printer'; status = 'not-run'; reason = 'No physical printer attached to acceptance VM.' }
    $result.status = 'passed'
    Save-Screenshot 'completed'
} catch {
    $result.status = 'failed'
    $result.error = $_.Exception.Message
    $result.stack = $_.ScriptStackTrace
    try { Save-Screenshot 'failure' } catch { Write-Warning $_ }
} finally {
    Save-Results
    Stop-Transcript | Out-Null
}
