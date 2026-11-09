# Regenerates the vector logo and Windows icon set using built-in Windows drawing APIs.
$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName System.Drawing
$assetPath = Join-Path (Split-Path $PSScriptRoot -Parent) 'assets'
New-Item -ItemType Directory -Path $assetPath -Force | Out-Null
$body = Get-Content -Raw -LiteralPath (Join-Path $assetPath 'logo-body.svg')
$svg = '<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 256 256" width="256" height="256" role="img" aria-label="Drone Deck">' + $body + '</svg>'
[IO.File]::WriteAllText((Join-Path $assetPath 'drone-deck.svg'), $svg)

# Match the two paths in logo-body.svg. Render at 4x, then downsample for small sizes.
function New-LogoBitmap([int]$size) {
    $large = [Drawing.Bitmap]::new($size * 4, $size * 4)
    $graphics = [Drawing.Graphics]::FromImage($large)
    $graphics.Clear([Drawing.Color]::Transparent)
    $graphics.SmoothingMode = [Drawing.Drawing2D.SmoothingMode]::AntiAlias
    $graphics.ScaleTransform($size / 64.0, $size / 64.0)
    $background = [Drawing.Drawing2D.GraphicsPath]::new()
    $background.AddArc(8, 8, 104, 104, 180, 90)
    $background.AddArc(144, 8, 104, 104, 270, 90)
    $background.AddArc(144, 144, 104, 104, 0, 90)
    $background.AddArc(8, 144, 104, 104, 90, 90)
    $background.CloseFigure()
    $brush = [Drawing.SolidBrush]::new([Drawing.Color]::FromArgb(17,17,17))
    $graphics.FillPath($brush, $background)
    $letter = [Drawing.Drawing2D.GraphicsPath]::new([Drawing.Drawing2D.FillMode]::Alternate)
    $letter.AddLine(76,62,130,62)
    $letter.AddBezier(130,62,175,62,202,88,202,128)
    $letter.AddBezier(202,128,202,168,175,194,130,194)
    $letter.AddLine(130,194,76,194)
    $letter.CloseFigure()
    $letter.StartFigure()
    $letter.AddLine(108,94,108,162)
    $letter.AddLine(108,162,130,162)
    $letter.AddBezier(130,162,155,162,170,150,170,128)
    $letter.AddBezier(170,128,170,106,155,94,130,94)
    $letter.CloseFigure()
    $graphics.FillPath([Drawing.Brushes]::White, $letter)
    $graphics.Dispose(); $background.Dispose(); $letter.Dispose(); $brush.Dispose()
    $result = [Drawing.Bitmap]::new($size, $size)
    $scaled = [Drawing.Graphics]::FromImage($result)
    $scaled.InterpolationMode = [Drawing.Drawing2D.InterpolationMode]::HighQualityBicubic
    $scaled.PixelOffsetMode = [Drawing.Drawing2D.PixelOffsetMode]::HighQuality
    $scaled.DrawImage($large, 0, 0, $size, $size)
    $scaled.Dispose(); $large.Dispose()
    return $result
}

$sizes = @(16,20,24,32,40,48,64,128,256)
$images = @()
foreach ($size in $sizes) {
    $bitmap = New-LogoBitmap $size
    $memory = [IO.MemoryStream]::new()
    $bitmap.Save($memory, [Drawing.Imaging.ImageFormat]::Png)
    $images += ,$memory.ToArray()
    if ($size -eq 256) { [IO.File]::WriteAllBytes((Join-Path $assetPath 'drone-deck.png'), $memory.ToArray()) }
    $bitmap.Dispose(); $memory.Dispose()
}
# ICO directory with PNG-compressed frames supported by Windows Vista and newer.
$file = [IO.File]::Create((Join-Path $assetPath 'drone-deck.ico'))
$writer = [IO.BinaryWriter]::new($file)
try {
    $writer.Write([uint16]0); $writer.Write([uint16]1); $writer.Write([uint16]$sizes.Count)
    $offset = 6 + 16 * $sizes.Count
    for ($i = 0; $i -lt $sizes.Count; $i++) {
        $dimension = if ($sizes[$i] -eq 256) { 0 } else { $sizes[$i] }
        $writer.Write([byte]$dimension); $writer.Write([byte]$dimension)
        $writer.Write([byte]0); $writer.Write([byte]0)
        $writer.Write([uint16]1); $writer.Write([uint16]32)
        $writer.Write([uint32]$images[$i].Length); $writer.Write([uint32]$offset)
        $offset += $images[$i].Length
    }
    foreach ($bytes in $images) { $writer.Write([byte[]]$bytes) }
} finally { $writer.Dispose() }
Write-Output 'Generated SVG, PNG, and nine-size ICO without additional packages.'
