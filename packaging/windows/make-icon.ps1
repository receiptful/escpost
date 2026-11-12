param(
    [string]$Source = (Join-Path $PSScriptRoot '..\..\assets\escpost-mark-simple.png'),
    [string]$Destination = (Join-Path $PSScriptRoot 'escpost.ico'),
    [string]$PreviewDirectory
)
# Run in the Compose-managed Windows builder. This only resamples/encodes the
# approved artwork; generation details live beside the PNG master.
$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName System.Drawing
$sizes = @(16, 24, 32, 48, 64, 128, 256)
$frames = New-Object 'Collections.Generic.List[byte[]]'
$sourceImage = [Drawing.Image]::FromFile((Resolve-Path $Source))
try {
    if ($sourceImage.Width -ne $sourceImage.Height) { throw 'The icon master must be square.' }
    if ($PreviewDirectory) { New-Item -ItemType Directory -Force $PreviewDirectory | Out-Null }
    foreach ($size in $sizes) {
        $bitmap = New-Object Drawing.Bitmap $size, $size
        $graphics = [Drawing.Graphics]::FromImage($bitmap)
        $stream = New-Object IO.MemoryStream
        try {
            $graphics.CompositingMode = [Drawing.Drawing2D.CompositingMode]::SourceCopy
            $graphics.InterpolationMode = [Drawing.Drawing2D.InterpolationMode]::HighQualityBicubic
            $graphics.PixelOffsetMode = [Drawing.Drawing2D.PixelOffsetMode]::HighQuality
            $graphics.DrawImage($sourceImage, 0, 0, $size, $size)
            $bitmap.Save($stream, [Drawing.Imaging.ImageFormat]::Png)
            $frames.Add($stream.ToArray())
            if ($PreviewDirectory) {
                $bitmap.Save((Join-Path $PreviewDirectory "icon-$size.png"), [Drawing.Imaging.ImageFormat]::Png)
            }
        } finally { $stream.Dispose(); $graphics.Dispose(); $bitmap.Dispose() }
    }
} finally { $sourceImage.Dispose() }
$output = [IO.File]::Create($Destination)
$writer = New-Object IO.BinaryWriter $output
try {
    $writer.Write([uint16]0)
    $writer.Write([uint16]1)
    $writer.Write([uint16]$sizes.Count)
    $offset = 6 + 16 * $sizes.Count
    for ($index = 0; $index -lt $sizes.Count; $index++) {
        $dimension = if ($sizes[$index] -eq 256) { 0 } else { $sizes[$index] }
        $writer.Write([byte]$dimension)
        $writer.Write([byte]$dimension)
        $writer.Write([byte]0)
        $writer.Write([byte]0)
        $writer.Write([uint16]1)
        $writer.Write([uint16]32)
        $writer.Write([uint32]$frames[$index].Length)
        $writer.Write([uint32]$offset)
        $offset += $frames[$index].Length
    }
    foreach ($frame in $frames) { $writer.Write([byte[]]$frame) }
} finally { $writer.Dispose(); $output.Dispose() }
Write-Host "Wrote $Destination ($($sizes -join ', ') pixels)."
