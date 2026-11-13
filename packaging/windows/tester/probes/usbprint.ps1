param(
    [Parameter(Mandatory = $true)][string]$Executable,
    [Parameter(Mandatory = $true)][ValidatePattern('^[0-9a-fA-F]{4}$')][string]$VendorId,
    [Parameter(Mandatory = $true)][ValidatePattern('^[0-9a-fA-F]{4}$')][string]$ProductId,
    [Parameter(Mandatory = $true)][string]$OutputDirectory,
    [switch]$Print
)
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
New-Item -ItemType Directory -Force $OutputDirectory | Out-Null
Start-Transcript -Path (Join-Path $OutputDirectory 'probe.log') | Out-Null
$env:DO_NOT_TRACK = '1'
$identity = [Security.Principal.WindowsIdentity]::GetCurrent()
$principal = New-Object Security.Principal.WindowsPrincipal($identity)
$report = [ordered]@{
    user = $identity.Name
    administrator = $principal.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)
    started = [DateTime]::UtcNow.ToString('o')
    spooler = (Get-Service Spooler).Status.ToString()
    executable_sha256 = (Get-FileHash $Executable -Algorithm SHA256).Hash
    vendor = $VendorId
    product = $ProductId
    status = 'failed'
    tests = @()
}
try {
    if ($report.administrator) { throw 'Run this probe as the standard Tester user.' }
    $devices = @(Get-PnpDevice -PresentOnly | Where-Object {
        $_.InstanceId -like "USB\VID_$VendorId&PID_$ProductId\*" -or
        $_.InstanceId -like "USB\VID_$VendorId&PID_$ProductId&MI_*"
    })
    $candidates = @($devices | ForEach-Object {
        $device = $_
        $service = (Get-PnpDeviceProperty -InstanceId $device.InstanceId -KeyName DEVPKEY_Device_Service).Data
        if ($service -eq 'usbprint') {
            [pscustomobject]@{ name = $device.FriendlyName; instance = $device.InstanceId; service = $service }
        }
    })
    $report.devices = $candidates
    if ($candidates.Count -ne 1) { throw "Expected exactly one USBPRINT interface, found $($candidates.Count)." }
    if ($Print) {
        Write-Host 'Registering through the normal USB discovery path.'
        $config = Join-Path $OutputDirectory 'printers.toml'
        # Exercise normal discovery and registration. No private device path or
        # backend flag is supplied to ESCPost, and the user's config is untouched.
        $registration = Start-Process -FilePath $Executable -ArgumentList @('printers', '--config', ('"' + $config + '"'), 'add', 'usbprint-test', '--transport', 'usb', '--vendor-id', "0x$VendorId", '--product-id', "0x$ProductId", '--non-interactive') -NoNewWindow -Wait -PassThru -RedirectStandardError (Join-Path $OutputDirectory 'registration.log')
        if ($registration.ExitCode -ne 0) { throw "Registration failed: $(Get-Content (Join-Path $OutputDirectory 'registration.log') -Raw)" }
        # Get-Content attaches provider metadata to its string in PowerShell 5;
        # serializing that metadata can walk the filesystem/provider graph.
        $report.configuration = [IO.File]::ReadAllText($config)
        $label = "$VendorId`:$ProductId / spooler $($report.spooler)"
        foreach ($mode in @('file', 'stream')) {
            Write-Host "Starting $mode test."
            $test = [ordered]@{ mode = $mode; writes = @(); status = 'failed' }
            $process = New-Object Diagnostics.Process
            $started = $false
            try {
                $start = New-Object Diagnostics.ProcessStartInfo
                $start.FileName = $Executable
                $start.UseShellExecute = $false
                $start.CreateNoWindow = $true
                $start.RedirectStandardInput = $true
                $start.RedirectStandardOutput = $true
                $start.RedirectStandardError = $true
                $start.Arguments = 'print --non-interactive --printer usbprint-test --config "' + $config + '"'
                $expectedBytes = 0
                if ($mode -eq 'file') {
                    $receipt = [Text.Encoding]::ASCII.GetBytes(([char]27) + "@ESCPost USBPRINT file`n$label`n`n`n")
                    $file = Join-Path $OutputDirectory 'receipt.bin'
                    [IO.File]::WriteAllBytes($file, $receipt)
                    $expectedBytes = $receipt.Length
                    $start.Arguments += ' "' + $file + '"'
                }
                # No explicit source for stream mode: exercise auto-stdin too.
                $process.StartInfo = $start
                if (-not $process.Start()) { throw 'Could not start ESCPost.' }
                $started = $true
                $stdout = $process.StandardOutput.ReadToEndAsync()
                $stderr = $process.StandardError.ReadToEndAsync()
                if ($mode -eq 'stream') {
                    foreach ($index in 1..3) {
                        $prefix = if ($index -eq 1) { ([char]27).ToString() + '@' } else { '' }
                        $bytes = [Text.Encoding]::ASCII.GetBytes("${prefix}USBPRINT stream $index/3`n$label`n`n`n")
                        $process.StandardInput.BaseStream.Write($bytes, 0, $bytes.Length)
                        $process.StandardInput.BaseStream.Flush()
                        $expectedBytes += $bytes.Length
                        $test.writes += @{ chunk = $index; bytes = $bytes.Length; time = [DateTime]::UtcNow.ToString('o') }
                        Start-Sleep -Seconds 2
                    }
                }
                $process.StandardInput.Close()
                Write-Host "$mode stdin closed; waiting for process."
                if (-not $process.WaitForExit(15000)) { throw 'ESCPost did not finish within 15 seconds after EOF.' }
                Write-Host "$mode exited: $($process.ExitCode)."
                $test.exit_code = $process.ExitCode
                if (-not $stdout.Wait(5000) -or -not $stderr.Wait(5000)) { throw 'ESCPost output pipes did not close after exit.' }
                $test.stdout = $stdout.Result
                $test.stderr = $stderr.Result
                $test.expected_bytes = $expectedBytes
                Write-Host "$mode output collected."
                if ($test.exit_code -ne 0) { throw "ESCPost failed: $($test.stderr)" }
                if ($test.stderr -notmatch "Bytes sent: $expectedBytes\b") { throw 'Byte summary did not match the input.' }
                $test.status = 'passed'
            } catch {
                $test.error = $_.Exception.Message
            } finally {
                if ($started -and -not $process.HasExited) { $process.Kill(); $process.WaitForExit() }
                $process.Dispose()
            }
            Write-Host "$mode result: $($test.status)."
            $report.tests += $test
        }
        Write-Host 'Starting workbench HTTP printing test.'
        $test = [ordered]@{ mode = 'http'; status = 'failed' }
        $server = $null
        try {
            $address = 'http://127.0.0.1:19079'
            # Refuse an occupied port rather than accidentally test another app.
            $listener = New-Object Net.Sockets.TcpListener([Net.IPAddress]::Loopback, 19079)
            $listener.Start()
            $listener.Stop()
            $server = Start-Process -FilePath $Executable -ArgumentList @('serve', '--config', ('"' + $config + '"'), '--web-listen', '127.0.0.1:19079', '--non-interactive') -NoNewWindow -PassThru -RedirectStandardOutput (Join-Path $OutputDirectory 'server.stdout') -RedirectStandardError (Join-Path $OutputDirectory 'server.stderr')
            $deadline = [DateTime]::UtcNow.AddSeconds(45)
            while ($true) {
                if ($server.HasExited) { throw 'The workbench server exited before becoming ready.' }
                try {
                    $null = Invoke-WebRequest "$address/health" -UseBasicParsing -TimeoutSec 2
                    break
                } catch {
                    if ([DateTime]::UtcNow -gt $deadline) { throw 'The workbench server did not become ready.' }
                    Start-Sleep -Milliseconds 250
                }
            }
            $bytes = [Text.Encoding]::ASCII.GetBytes(([char]27) + "@ESCPost USBPRINT HTTP`n$label`n`n`n")
            # Submit once: failed print requests must never be retried automatically.
            $response = Invoke-RestMethod "$address/api/print?printer=usbprint-test" -Method Post -ContentType 'application/octet-stream' -Body $bytes -TimeoutSec 20
            if (-not $response.job_id) { throw 'The print API returned no job ID.' }
            $test.job_id = $response.job_id
            $test.input_bytes = $bytes.Length
            $test.status = 'passed'
        } catch {
            $test.error = $_.Exception.Message
        } finally {
            if ($null -ne $server) {
                if (-not $server.HasExited) { $server.Kill(); $server.WaitForExit() }
                $server.Dispose()
            }
        }
        $report.tests += $test
        if (@($report.tests | Where-Object { $_.status -ne 'passed' }).Count) { throw 'One or more printing checks failed.' }
    }
    $report.status = 'passed'
} catch {
    $report.error = $_.Exception.Message
} finally {
    Write-Host 'Saving test evidence.'
    $report.finished = [DateTime]::UtcNow.ToString('o')
    $temporary = Join-Path $OutputDirectory 'results.tmp'
    $report | ConvertTo-Json -Depth 8 | Set-Content $temporary -Encoding UTF8
    Move-Item $temporary (Join-Path $OutputDirectory 'results.json') -Force
    Stop-Transcript | Out-Null
}
if ($report.status -ne 'passed') { exit 1 }
