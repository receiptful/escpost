$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
Add-Type -AssemblyName System.Drawing
Add-Type -AssemblyName System.Windows.Forms

function Save-Results {
    $result.updated = [DateTime]::UtcNow.ToString('o')
    $temporary = Join-Path $evidence 'results.partial.json'
    $result | ConvertTo-Json -Depth 12 | Set-Content $temporary -Encoding UTF8
    Move-Item $temporary (Join-Path $evidence 'results.json') -Force
}
function Check {
    param([string]$Name, [scriptblock]$Body)
    Write-Host "Checking $Name..."
    $started = [DateTime]::UtcNow
    try {
        & $Body | Out-Host
        $result.checks += @{ name = $Name; status = 'passed'; seconds = ([DateTime]::UtcNow - $started).TotalSeconds }
    } catch {
        $result.checks += @{ name = $Name; status = 'failed'; error = $_.Exception.Message }
        throw
    } finally { Save-Results }
}
function Assert-True {
    param([bool]$Condition, [string]$Message)
    if (-not $Condition) { throw $Message }
}
function Save-Screenshot {
    param([string]$Name)
    $bounds = [Windows.Forms.SystemInformation]::VirtualScreen
    $bitmap = New-Object Drawing.Bitmap $bounds.Width, $bounds.Height
    $graphics = [Drawing.Graphics]::FromImage($bitmap)
    try {
        $graphics.CopyFromScreen($bounds.Location, [Drawing.Point]::Empty, $bounds.Size)
        $bitmap.Save((Join-Path $evidence "$Name.png"), [Drawing.Imaging.ImageFormat]::Png)
    } finally { $graphics.Dispose(); $bitmap.Dispose() }
}
function Run-Setup {
    param([string]$File, [string]$Name, [string[]]$Options = @())
    $arguments = @('/VERYSILENT', '/SUPPRESSMSGBOXES', '/NORESTART', '/SP-', "/LOG=`"$evidence\$Name.log`"") + $Options
    $process = Start-Process -FilePath $File -PassThru -ArgumentList $arguments
    try {
        if (-not $process.WaitForExit(180000)) { throw "$Name timed out (PID $($process.Id))." }
        Assert-True ($process.ExitCode -eq 0) "$Name exited with $($process.ExitCode)."
    } finally { $process.Dispose() }
}
function Run-Cli {
    param([string[]]$Arguments)
    $info = New-Object Diagnostics.ProcessStartInfo
    $info.FileName = $exe
    $info.Arguments = ($Arguments | ForEach-Object { Quote-Argument $_ }) -join ' '
    $info.WorkingDirectory = $JobDirectory
    $info.UseShellExecute = $false
    $info.CreateNoWindow = $true
    $info.RedirectStandardOutput = $true
    $info.RedirectStandardError = $true
    $process = [Diagnostics.Process]::Start($info)
    $stdout = $process.StandardOutput.ReadToEndAsync()
    $stderr = $process.StandardError.ReadToEndAsync()
    try {
        if (-not $process.WaitForExit(30000)) { $process.Kill(); throw 'CLI timed out.' }
        $output = $stdout.GetAwaiter().GetResult() + $stderr.GetAwaiter().GetResult()
        $code = $process.ExitCode
    } finally { $process.Dispose() }
    $output | Out-File (Join-Path $evidence 'cli.log') -Append -Encoding UTF8
    if ($code -ne 0) { throw "CLI failed ($code): $output" }
    return ($output | Out-String).Trim()
}
function Quote-Argument {
    param([string]$Value)
    $quoted = '"'
    $slashes = 0
    foreach ($character in $Value.ToCharArray()) {
        if ($character -eq '\') { $slashes++; continue }
        if ($character -eq '"') { $quoted += ('\' * ($slashes * 2 + 1)) }
        else { $quoted += ('\' * $slashes) }
        $quoted += $character
        $slashes = 0
    }
    return $quoted + ('\' * ($slashes * 2)) + '"'
}
function User-Path {
    $key = [Microsoft.Win32.Registry]::CurrentUser.OpenSubKey('Environment')
    try { return [string]$key.GetValue('Path', '', [Microsoft.Win32.RegistryValueOptions]::DoNotExpandEnvironmentNames) }
    finally { $key.Dispose() }
}
function Set-UserPath {
    param([string]$Value)
    $key = [Microsoft.Win32.Registry]::CurrentUser.CreateSubKey('Environment')
    try { $key.SetValue('Path', $Value, [Microsoft.Win32.RegistryValueKind]::ExpandString) }
    finally { $key.Dispose() }
}
function Assert-PathCount {
    param([int]$Count)
    $entries = @((User-Path).Split(';') | Where-Object { $_.Trim().Trim('"').TrimEnd('\') -ieq $installDir })
    Assert-True ($entries.Count -eq $Count) "Expected $Count installed PATH entries, got $($entries.Count)."
}
function Wait-Until {
    param([scriptblock]$Condition, [string]$Message, [int]$Seconds = 30)
    $deadline = [DateTime]::UtcNow.AddSeconds($Seconds)
    do {
        if (& $Condition) { return }
        Start-Sleep -Milliseconds 250
    } while ([DateTime]::UtcNow -lt $deadline)
    throw $Message
}
