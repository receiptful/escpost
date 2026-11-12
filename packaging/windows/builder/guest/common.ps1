$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest

$builderRoot = 'C:\EscpostBuilder'
$env:CARGO_HOME = Join-Path $builderRoot 'cargo'
$env:RUSTUP_HOME = Join-Path $builderRoot 'rustup'
$env:CARGO_TARGET_DIR = Join-Path $builderRoot 'target'
$env:PATH = "$env:CARGO_HOME\bin;$env:PATH"
$toolchain = Get-Content (Join-Path $PSScriptRoot 'toolchain.json') -Raw | ConvertFrom-Json
$cargo = Join-Path $env:CARGO_HOME 'bin\cargo.exe'
$rustup = Join-Path $env:CARGO_HOME 'bin\rustup.exe'
$vsRoot = Join-Path $builderRoot 'BuildTools'
$iscc = Join-Path $builderRoot "InnoSetup-$($toolchain.inno.version)\ISCC.exe"

function Invoke-Checked {
    param([string]$Program, [string[]]$Arguments)
    & $Program @Arguments
    if ($LASTEXITCODE -ne 0) { throw "$Program failed with exit code $LASTEXITCODE" }
}

function Get-Installer {
    param([string]$Name, $Source)
    $cache = Join-Path $builderRoot "downloads\$Name-$($Source.sha256)"
    New-Item -ItemType Directory -Force -Path $cache | Out-Null
    # Rustup dispatches by argv[0]; renaming rustup-init.exe makes it act as a
    # compiler proxy. Put the hash in the directory, preserving upstream names.
    $path = Join-Path $cache ([IO.Path]::GetFileName(([Uri]$Source.url).AbsolutePath))
    if (-not (Test-Path $path)) {
        $partial = "$path.partial"
        Invoke-Checked 'curl.exe' @('--fail', '--location', '--retry', '3', '--output', $partial, $Source.url)
        if ((Get-FileHash $partial -Algorithm SHA256).Hash -ne $Source.sha256) {
            Remove-Item $partial -Force
            throw "Checksum mismatch downloading $Name"
        }
        Move-Item $partial $path -Force
    }
    if ((Get-FileHash $path -Algorithm SHA256).Hash -ne $Source.sha256) {
        throw "Cached $Name checksum mismatch; remove $path and retry."
    }
    return $path
}

function Install-Checked {
    param([string]$Program, [string[]]$Arguments)
    $process = Start-Process -FilePath $Program -ArgumentList $Arguments -Wait -PassThru
    if ($process.ExitCode -eq 3010) {
        throw 'Build tools require a Windows restart. Restart the builder VM and rerun the build.'
    }
    if ($process.ExitCode -ne 0) { throw "$Program failed with exit code $($process.ExitCode)" }
}

function Import-VisualStudioEnvironment {
    $devShell = Join-Path $vsRoot 'Common7\Tools\Launch-VsDevShell.ps1'
    & $devShell -Arch amd64 -HostArch amd64 -SkipAutomaticLocation | Out-Null
}
