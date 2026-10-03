# Снимок экрана (или области) в PNG — для проверки окна программы.
#   .\scripts\screenshot.ps1 -Out shot.png [-X 0 -Y 0 -W 800 -H 600]
param([Parameter(Mandatory)][string]$Out, [int]$X = -1, [int]$Y = 0, [int]$W = 0, [int]$H = 0)
Add-Type -AssemblyName System.Windows.Forms, System.Drawing
Add-Type @"
using System; using System.Runtime.InteropServices;
public static class Dpi { [DllImport("user32.dll")] public static extern bool SetProcessDPIAware(); }
"@
[Dpi]::SetProcessDPIAware() | Out-Null
$b = [System.Windows.Forms.Screen]::PrimaryScreen.Bounds
if ($X -lt 0) { $X = $b.X; $Y = $b.Y; $W = $b.Width; $H = $b.Height }
$bmp = New-Object System.Drawing.Bitmap $W, $H
$g = [System.Drawing.Graphics]::FromImage($bmp)
$g.CopyFromScreen($X, $Y, 0, 0, $bmp.Size)
$bmp.Save($Out, [System.Drawing.Imaging.ImageFormat]::Png)
$g.Dispose(); $bmp.Dispose()
"$W x $H -> $Out"
