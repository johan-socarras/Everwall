# Draws Everwall's icon (three screens: one portrait and two landscape) and
# saves it as src-tauri\icons\icon.ico in several sizes. Run once; the .ico
# is kept in the repository.
#   pwsh -File tools\make-icon.ps1
Add-Type -AssemblyName System.Drawing

$target = Join-Path $PSScriptRoot '..\src-tauri\icons\icon.ico'
$target = [IO.Path]::GetFullPath($target)
New-Item -ItemType Directory -Force (Split-Path $target) | Out-Null

function New-RoundedRectangle([float]$x, [float]$y, [float]$w, [float]$h, [float]$r) {
    $p = New-Object System.Drawing.Drawing2D.GraphicsPath
    $d = 2 * $r
    $p.AddArc($x, $y, $d, $d, 180, 90)
    $p.AddArc($x + $w - $d, $y, $d, $d, 270, 90)
    $p.AddArc($x + $w - $d, $y + $h - $d, $d, $d, 0, 90)
    $p.AddArc($x, $y + $h - $d, $d, $d, 90, 90)
    $p.CloseFigure()
    return $p
}

function Draw-Icon([int]$size) {
    $bmp = New-Object System.Drawing.Bitmap $size, $size, ([System.Drawing.Imaging.PixelFormat]::Format32bppArgb)
    $g = [System.Drawing.Graphics]::FromImage($bmp)
    $g.SmoothingMode = 'AntiAlias'
    $g.Clear([System.Drawing.Color]::Transparent)
    $k = $size / 256.0

    $background = New-RoundedRectangle (8 * $k) (8 * $k) (240 * $k) (240 * $k) (56 * $k)
    $grad = New-Object System.Drawing.Drawing2D.LinearGradientBrush(
        (New-Object System.Drawing.PointF 0, 0),
        (New-Object System.Drawing.PointF $size, $size),
        [System.Drawing.Color]::FromArgb(255, 109, 93, 252),
        [System.Drawing.Color]::FromArgb(255, 43, 179, 255))
    $g.FillPath($grad, $background)

    # x, y, width, height on the 256 grid: 44 + 8 + 70 + 8 + 70 = 200 wide,
    # centred (28 on each side), and each screen centred vertically
    $screens = @(
        @(28, 88, 44, 80),
        @(80, 106, 70, 44),
        @(158, 106, 70, 44)
    )
    $white = New-Object System.Drawing.SolidBrush ([System.Drawing.Color]::FromArgb(242, 255, 255, 255))
    foreach ($p in $screens) {
        $r = [Math]::Max(1.0, 7 * $k)
        $path = New-RoundedRectangle ($p[0] * $k) ($p[1] * $k) ($p[2] * $k) ($p[3] * $k) $r
        $g.FillPath($white, $path)
    }
    $g.Dispose()
    $ms = New-Object IO.MemoryStream
    $bmp.Save($ms, [System.Drawing.Imaging.ImageFormat]::Png)
    $bmp.Dispose()
    return , $ms.ToArray()
}

$sizes = 16, 24, 32, 48, 64, 128, 256
$images = foreach ($t in $sizes) { , (Draw-Icon $t) }

$output = New-Object IO.MemoryStream
$bw = New-Object IO.BinaryWriter $output
$bw.Write([uint16]0); $bw.Write([uint16]1); $bw.Write([uint16]$sizes.Count)
$offset = 6 + 16 * $sizes.Count
for ($i = 0; $i -lt $sizes.Count; $i++) {
    $t = $sizes[$i]; $data = $images[$i]
    $side = if ($t -ge 256) { 0 } else { $t }
    $bw.Write([byte]$side); $bw.Write([byte]$side); $bw.Write([byte]0); $bw.Write([byte]0)
    $bw.Write([uint16]1); $bw.Write([uint16]32)
    $bw.Write([uint32]$data.Length); $bw.Write([uint32]$offset)
    $offset += $data.Length
}
foreach ($data in $images) { $bw.Write($data) }
$bw.Flush()
[IO.File]::WriteAllBytes($target, $output.ToArray())
# A PNG copy, to look at it without opening the .ico
[IO.File]::WriteAllBytes(([IO.Path]::ChangeExtension($target, '.png')), $images[-1])
"Icon written to $target"
