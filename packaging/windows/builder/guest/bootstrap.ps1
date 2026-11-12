. (Join-Path $PSScriptRoot 'common.ps1')

foreach ($script in Get-ChildItem $PSScriptRoot -Filter '*.ps1') {
    $tokens = $null
    $parseErrors = $null
    [void][System.Management.Automation.Language.Parser]::ParseFile($script.FullName, [ref]$tokens, [ref]$parseErrors)
    if ($parseErrors.Count) { throw ($parseErrors | Out-String) }
}

New-Item -ItemType Directory -Force -Path $builderRoot | Out-Null
Set-ItemProperty 'HKLM:\SYSTEM\CurrentControlSet\Control\FileSystem' -Name LongPathsEnabled -Value 1
$vswhere = "${env:ProgramFiles(x86)}\Microsoft Visual Studio\Installer\vswhere.exe"
$installedVersion = ''
if (Test-Path $vswhere) {
    $installedVersion = & $vswhere -products Microsoft.VisualStudio.Product.BuildTools `
        -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 $toolchain.visualStudio.sdkComponent `
        -property installationVersion
}
if ($installedVersion -ne $toolchain.visualStudio.version -or -not (Test-Path "$vsRoot\Common7\Tools\Launch-VsDevShell.ps1")) {
    if (Test-Path "$vsRoot\Common7\Tools\Launch-VsDevShell.ps1") {
        throw 'The existing MSVC installation differs from toolchain.json. Use a fresh builder VM for this toolchain.'
    }
    Write-Host "Installing MSVC Build Tools $($toolchain.visualStudio.version)..."
    $installer = Get-Installer 'vs-buildtools' $toolchain.visualStudio
    Install-Checked $installer @('--quiet', '--wait', '--norestart', '--nocache', '--installPath', $vsRoot,
        '--add', 'Microsoft.VisualStudio.Component.VC.Tools.x86.x64',
        '--add', $toolchain.visualStudio.sdkComponent)
    $installedVersion = & $vswhere -products Microsoft.VisualStudio.Product.BuildTools -property installationVersion
    if ($installedVersion -ne $toolchain.visualStudio.version) { throw "Unexpected MSVC version: $installedVersion" }
}

if (-not (Test-Path $rustup)) {
    Write-Host 'Installing Rustup...'
    $installer = Get-Installer 'rustup' $toolchain.rustup
    Invoke-Checked $installer @('-y', '--no-modify-path', '--profile', 'minimal',
        '--default-host', 'x86_64-pc-windows-msvc', '--default-toolchain', $toolchain.rust)
}
Invoke-Checked $rustup @('set', 'auto-self-update', 'disable')
Invoke-Checked $rustup @('toolchain', 'install', $toolchain.rust, '--profile', 'minimal', '--component', 'clippy,rustfmt')

if (-not (Test-Path $iscc)) {
    Write-Host "Installing Inno Setup $($toolchain.inno.version)..."
    $installer = Get-Installer 'innosetup' $toolchain.inno
    Install-Checked $installer @('/VERYSILENT', '/SUPPRESSMSGBOXES', '/NORESTART', '/CURRENTUSER', "/DIR=$(Split-Path $iscc)")
}
# ISCC's PE version is 0.0.0.0; the installer records the release version here.
$innoRegistration = Get-ItemProperty 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\Inno Setup 6_is1'
if ($innoRegistration.DisplayVersion -ne $toolchain.inno.version -or
    $innoRegistration.InstallLocation.TrimEnd('\') -ne (Split-Path $iscc)) {
    throw 'Unexpected Inno Setup version.'
}
Invoke-Checked $cargo @("+$($toolchain.rust)", '--version')
Write-Host "MSVC: $installedVersion; Inno Setup: $($toolchain.inno.version)"
