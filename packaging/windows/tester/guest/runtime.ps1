function Test-InstalledRuntime {
    $receipt = Join-Path $JobDirectory 'receipt.hex'
    # Reference profile is 576 dots wide. ESC * draws eight vertical black dots
    # at the left edge, followed by printable text and line feeds.
    '1b 40 1b 2a 01 01 00 ff 0a 45 53 43 50 6f 73 74 0a 0a 0a' | Set-Content $receipt -Encoding Ascii
    Check 'installed renderer and bundled profile/font' {
        Run-Cli @('render', $receipt, '--profile', 'REFERENCE', '--output-dir', "$JobDirectory\rendered", '--non-interactive')
        $images = @(Get-ChildItem "$JobDirectory\rendered" -Filter '*.png')
        Assert-True ($images.Count -eq 1) 'Expected one PNG sheet.'
        $bitmap = [Drawing.Bitmap]::FromFile($images[0].FullName)
        try {
            Assert-True ($bitmap.Width -eq 576 -and $bitmap.Height -gt 0) 'Unexpected rendered dimensions.'
            $black = 0
            for ($y = 0; $y -lt $bitmap.Height; $y++) {
                for ($x = 0; $x -lt $bitmap.Width; $x++) {
                    if ($bitmap.GetPixel($x, $y).R -lt 128) { $black++ }
                }
            }
            Assert-True ($black -gt 8) 'Rendered receipt has no text/graphics.'
        } finally { $bitmap.Dispose() }
        Copy-Item $images[0].FullName "$evidence\rendered-receipt.png"
    }
    Check 'RAW TCP exact-byte output' {
        $listener = New-Object Net.Sockets.TcpListener ([Net.IPAddress]::Loopback), 0
        $listener.Start()
        $client = $null
        try {
            $accept = $listener.AcceptTcpClientAsync()
            $payload = [byte[]](0x1b, 0x40, 0x00, 0xff, 0x41, 0x0a)
            [IO.File]::WriteAllBytes("$JobDirectory\raw.bin", $payload)
            Run-Cli @('print', "$JobDirectory\raw.bin", '--network', "127.0.0.1:$($listener.LocalEndpoint.Port)", '--non-interactive')
            Assert-True ($accept.Wait(10000)) 'Printer connection never arrived.'
            $client = $accept.Result
            $client.ReceiveTimeout = 10000
            $stream = $client.GetStream()
            $received = New-Object IO.MemoryStream
            try { $stream.CopyTo($received); $bytes = $received.ToArray() }
            finally { $received.Dispose() }
            Assert-True ([BitConverter]::ToString($bytes) -ceq [BitConverter]::ToString($payload)) 'Printing changed raw bytes.'
        } finally { if ($client) { $client.Dispose() }; $listener.Stop() }
    }
    $stderr = Join-Path $evidence 'server-stderr.log'
    $process = Start-Process -FilePath $exe -PassThru -WorkingDirectory $JobDirectory -ArgumentList @('serve', '--listen', '127.0.0.1:0', '--web-listen', '127.0.0.1:0', '--non-interactive', '--no-open') -RedirectStandardOutput "$evidence\server-stdout.log" -RedirectStandardError $stderr
    try {
        $script:address = $null
        Check 'installed HTTP server and embedded assets' {
            Wait-Until {
                if ($process.HasExited) { throw "Server exited: $(Get-Content $stderr -Raw)" }
                $log = Get-Content $stderr -Raw -ErrorAction SilentlyContinue
                if ($log -match 'http://127\.0\.0\.1:(\d+)') { $script:address = "http://127.0.0.1:$($Matches[1])"; return $true }
                return $false
            } 'Server did not report its web address.'
            Wait-Until {
                if ($process.HasExited) { throw "Server exited: $(Get-Content $stderr -Raw)" }
                try { return (Invoke-WebRequest "$script:address/health" -UseBasicParsing -TimeoutSec 2).StatusCode -eq 200 }
                catch { return $false }
            } 'HTTP server did not become healthy.'
            $page = Invoke-WebRequest "$script:address/" -UseBasicParsing -TimeoutSec 10
            Assert-True ($page.Content -match '(?:src|href)="(/assets/[^" ]+\.js)"') 'Embedded JavaScript missing.'
            $asset = Invoke-WebRequest "$script:address$($Matches[1])" -UseBasicParsing -TimeoutSec 10
            Assert-True ($asset.StatusCode -eq 200 -and $asset.RawContentLength -gt 0) 'Embedded asset failed.'
        }
        . "$PSScriptRoot\browser.ps1"
        Check 'Edge renders the installed workbench without JavaScript errors' { Test-Browser $script:address }
    } finally {
        if (-not $process.HasExited) { Stop-Process -Id $process.Id -Force }
        $process.Dispose()
    }
}
