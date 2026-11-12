function Send-Cdp {
    param([string]$Method, [hashtable]$Parameters = @{})
    $script:cdpId++
    $message = @{ id = $script:cdpId; method = $Method; params = $Parameters } | ConvertTo-Json -Compress -Depth 10
    $bytes = [Text.Encoding]::UTF8.GetBytes($message)
    $cancellation = New-Object Threading.CancellationTokenSource 15000
    try {
        [void]$script:cdpSocket.SendAsync([ArraySegment[byte]]::new($bytes), [Net.WebSockets.WebSocketMessageType]::Text, $true, $cancellation.Token).GetAwaiter().GetResult()
        do {
            $buffer = New-Object byte[] 65536
            $stream = New-Object IO.MemoryStream
            try {
                do {
                    $frame = $script:cdpSocket.ReceiveAsync([ArraySegment[byte]]::new($buffer), $cancellation.Token).GetAwaiter().GetResult()
                    if ($frame.MessageType -eq [Net.WebSockets.WebSocketMessageType]::Close) { throw 'Browser closed DevTools connection.' }
                    $stream.Write($buffer, 0, $frame.Count)
                } until ($frame.EndOfMessage)
                $reply = [Text.Encoding]::UTF8.GetString($stream.ToArray()) | ConvertFrom-Json
            } finally { $stream.Dispose() }
            if ($reply.PSObject.Properties['method']) {
                if ($reply.method -eq 'Runtime.exceptionThrown' -or ($reply.method -eq 'Log.entryAdded' -and $reply.params.entry.level -eq 'error')) {
                    $script:browserErrors += ($reply | ConvertTo-Json -Compress -Depth 10)
                }
            }
        } until ($reply.PSObject.Properties['id'] -and $reply.id -eq $script:cdpId)
        if ($reply.PSObject.Properties['error']) { throw ($reply.error | ConvertTo-Json -Compress) }
        return $reply.result
    } finally { $cancellation.Dispose() }
}
function Test-Browser {
    param([string]$Address)
    $edge = "${env:ProgramFiles(x86)}\Microsoft\Edge\Application\msedge.exe"
    Assert-True (Test-Path $edge) 'Windows Edge is missing.'
    $profile = Join-Path $JobDirectory 'edge-profile'
    $arguments = @('--no-first-run', '--no-default-browser-check', '--remote-debugging-port=0', '--remote-debugging-address=127.0.0.1', "--user-data-dir=$profile", 'about:blank')
    $edgeProcess = Start-Process $edge -PassThru -ArgumentList (($arguments | ForEach-Object { Quote-Argument $_ }) -join ' ')
    $script:cdpSocket = New-Object Net.WebSockets.ClientWebSocket
    $script:cdpId = 0
    $script:browserErrors = @()
    try {
        Wait-Until { Test-Path "$profile\DevToolsActivePort" } 'Edge DevTools did not start.'
        $port = (Get-Content "$profile\DevToolsActivePort")[0]
        $pages = Invoke-RestMethod "http://127.0.0.1:$port/json/list" -TimeoutSec 10
        $page = @($pages | Where-Object type -eq page)[0]
        $cancellation = New-Object Threading.CancellationTokenSource 10000
        try { [void]$script:cdpSocket.ConnectAsync([Uri]$page.webSocketDebuggerUrl, $cancellation.Token).GetAwaiter().GetResult() }
        finally { $cancellation.Dispose() }
        Send-Cdp 'Runtime.enable' | Out-Null
        Send-Cdp 'Log.enable' | Out-Null
        Send-Cdp 'Page.enable' | Out-Null
        Send-Cdp 'Page.navigate' @{ url = $Address } | Out-Null
        Wait-Until {
            $evaluation = Send-Cdp 'Runtime.evaluate' @{ expression = 'document.readyState === "complete" && !!document.querySelector("nav[aria-label=\"Workbench navigation\"]") && document.body.innerText.includes("Workbench")'; returnByValue = $true }
            return ($evaluation.result.PSObject.Properties['value'] -and $evaluation.result.value -eq $true)
        } 'Workbench did not render in Edge.' 45
        $state = Send-Cdp 'Runtime.evaluate' @{ expression = 'JSON.stringify({url:location.href,title:document.title,text:document.body.innerText})'; returnByValue = $true }
        $state.result.value | Set-Content "$evidence\browser-page.json" -Encoding UTF8
        $screenshot = Send-Cdp 'Page.captureScreenshot' @{ format = 'png' }
        [IO.File]::WriteAllBytes("$evidence\browser.png", [Convert]::FromBase64String($screenshot.data))
        Set-Content "$evidence\browser-errors.log" -Value ($script:browserErrors -join [Environment]::NewLine) -Encoding UTF8
        Assert-True ($script:browserErrors.Count -eq 0) "Browser errors: $script:browserErrors"
        try { Send-Cdp 'Browser.close' | Out-Null } catch { Write-Host 'Edge closed its DevTools connection.' }
    } finally {
        $script:cdpSocket.Dispose()
        if (-not $edgeProcess.HasExited) { Stop-Process -Id $edgeProcess.Id -Force -ErrorAction SilentlyContinue }
        $edgeProcess.Dispose()
    }
}
