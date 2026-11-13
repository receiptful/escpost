param(
    [Parameter(Mandatory = $true)]
    [ValidatePattern('^[a-zA-Z0-9_-]+$')]
    [string]$JobId
)
. (Join-Path $PSScriptRoot 'common.ps1')

$incoming = Join-Path $env:USERPROFILE "incoming\$JobId"
$job = Join-Path $builderRoot "jobs\$JobId"
$source = Join-Path $job 'source'
$artifacts = Join-Path $env:USERPROFILE "artifacts\$JobId"
$version = (Get-Content (Join-Path $incoming 'version.txt') -Raw).Trim()
if ($version -notmatch '^\d+\.\d+\.\d+(-dev\.[a-f0-9]+(-dirty)?)?$') { throw 'Invalid build version.' }
if (Test-Path $job) { throw "Build job already exists: $job" }
$expectedHash = ((Get-Content (Join-Path $incoming 'source.sha256') -Raw) -split '\s+')[0]
$archive = Join-Path $incoming 'source.tar.gz'
if ((Get-FileHash $archive -Algorithm SHA256).Hash -ne $expectedHash) { throw 'Source archive checksum mismatch.' }
New-Item -ItemType Directory -Force -Path $source, $artifacts | Out-Null
Invoke-Checked 'tar.exe' @('-xzf', $archive, '-C', $source)

Import-VisualStudioEnvironment
Get-CimInstance Win32_OperatingSystem | Select-Object Caption, Version, BuildNumber | Format-List
$target = 'x86_64-pc-windows-msvc'
$env:CARGO_TARGET_X86_64_PC_WINDOWS_MSVC_RUSTFLAGS = '-C target-feature=+crt-static'
$env:DO_NOT_TRACK = '1'
$env:ESCPOST_TELEMETRY = '0'
$env:CI = '1'
$env:ESCPOST_CONFIG_DIR = Join-Path $job 'config'

Push-Location $source
try {
    # Jobs use separate source snapshots but share a target directory. Cargo's
    # dep-info can otherwise keep pointing at a previous, retained snapshot.
    # Rebuild our crates; keep downloaded and compiled third-party dependencies.
    Invoke-Checked $cargo @("+$($toolchain.rust)", 'clean', '--target', $target,
        '-p', 'escpost', '-p', 'escpost-render', '-p', 'escpost-profiles', '-p', 'escpost-usbprint')
    Invoke-Checked $cargo @("+$($toolchain.rust)", 'test', '--locked', '--no-fail-fast', '--workspace', '--exclude', 'escpost-python', '--target', $target)
    Invoke-Checked $cargo @("+$($toolchain.rust)", 'build', '--locked', '--release', '-p', 'escpost', '--target', $target)
    $exe = Join-Path $env:CARGO_TARGET_DIR "$target\release\escpost.exe"
    # Inspect the imports even though the builder itself has VC runtimes installed.
    $imports = & dumpbin.exe /DEPENDENTS $exe
    if ($LASTEXITCODE -ne 0) { throw 'Could not inspect executable DLL imports.' }
    $imports | Set-Content (Join-Path $job 'dll-imports.txt')
    if ($imports -match '(?i)(VCRUNTIME|MSVCP|libusb|python\d).*\.dll') {
        throw 'The executable imports a non-system runtime DLL.'
    }
    & (Join-Path $PSScriptRoot 'smoke.ps1') -Executable $exe -Directory (Join-Path $job 'smoke')

    $prefix = "escpost-$version-windows-x64"
    $payload = Join-Path $job $prefix
    $licenses = Join-Path $payload 'licenses'
    New-Item -ItemType Directory -Force -Path $licenses | Out-Null
    Copy-Item $exe (Join-Path $payload 'escpost.exe')
    Copy-Item (Join-Path $source 'LICENSE') (Join-Path $payload 'LICENSE.txt')
    Copy-Item (Join-Path $source 'packaging\windows\README.txt') $payload
    Copy-Item (Join-Path $source 'crates\escpost-render\assets\fonts\noto-sans-mono\OFL.txt') (Join-Path $licenses 'NotoSansMono-OFL.txt')
    Copy-Item (Join-Path $source 'python\src\escpost\licenses\escpos-printer-db-LICENSE.md') $licenses

    $productVersion = $version.Split('-')[0]
    Invoke-Checked $iscc @("/DFullVersion=$version", "/DProductVersion=$productVersion", "/DSourceDir=$payload",
        "/O$artifacts", (Join-Path $source 'packaging\windows\escpost.iss'))
    & (Join-Path $PSScriptRoot 'test-installer.ps1') -Installer (Join-Path $artifacts "$prefix-setup.exe") -Directory (Join-Path $job 'installer-test')
    Compress-Archive -Path $payload -DestinationPath (Join-Path $artifacts "$prefix.zip")
    $checksums = foreach ($name in @("$prefix.zip", "$prefix-setup.exe")) {
        $hash = (Get-FileHash (Join-Path $artifacts $name) -Algorithm SHA256).Hash.ToLowerInvariant()
        "$hash  $name"
    }
    $checksums | Set-Content -Encoding Ascii (Join-Path $artifacts "$prefix-SHA256SUMS")
    Write-Host "Artifacts: $artifacts"
}
finally {
    Pop-Location
}
