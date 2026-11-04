<#
.SYNOPSIS
Screenshots and short webm clips of JobFinder's window, for the web page.

.DESCRIPTION
Start JobFinder first, and go to the screen you want. Then:

  .\tools\capture.ps1 shot dashboard            # site\media\dashboard-light.png and -dark.png
  .\tools\capture.ps1 shot task-manager -Theme dark
  .\tools\capture.ps1 clip match-why -Seconds 12  # site\media\match-why-light.webm

The window is set to one size first, so every picture lines up. "shot" takes the light and the
dark look by switching Windows' app colour mode for a moment (JobFinder follows it live) and
setting it back after. "clip" counts down 3 seconds, then records only the window while you click;
it needs ffmpeg (winget install Gyan.FFmpeg).

The phone app, over USB debugging (adb, from Android platform-tools):

  .\tools\capture.ps1 phone-shot matches       # site\media\phone-matches-light.png and -dark.png
  .\tools\capture.ps1 phone-clip send-a-post -Seconds 15

"phone-shot" switches the phone's dark theme the same way and sets it back. "phone-clip" records
the phone's screen and turns it into webm with ffmpeg.

Keep your CV, profile, letters, name, address and phone number out of the frame.
#>
param(
    [Parameter(Mandatory, Position = 0)][ValidateSet('shot', 'clip', 'phone-shot', 'phone-clip')][string]$What,
    [Parameter(Mandatory, Position = 1)][ValidatePattern('^[a-z0-9-]+$')][string]$Name,
    # Light, dark, or both (shots only; a clip records the look Windows has now).
    [ValidateSet('light', 'dark', 'both')][string]$Theme = 'both',
    # How long a clip records (the phone stops at 180).
    [ValidateRange(1, 180)][int]$Seconds = 10,
    # Where the files go.
    [string]$Out = (Join-Path $PSScriptRoot '..\site\media')
)
$ErrorActionPreference = 'Stop'

# The window's size on screen, in pixels, for every picture (16:10).
$WindowWidth = 1280
$WindowHeight = 800
# Frames per second of a clip.
$ClipFps = 30
# VP9 quality: lower is better and bigger (0-63); 32 keeps a 10 s clip around 1 MB.
$ClipCrf = 32
# Seconds to wait before a clip starts, to get your hand on the mouse.
$CountdownSeconds = 3
# Milliseconds for the window to redraw after a resize or a colour mode switch.
$SettleMs = 800
# Longest wait, in seconds, for the window to show the new light or dark look.
$LookTimeoutSeconds = 15
# Milliseconds between looks at the window while waiting for it to change.
$LookPollMs = 250
# Milliseconds between repeated colour-change notices while the window hasn't changed yet.
$NudgeMs = 1000
# Where to read the page colour: in the sidebar, under its last item, this far from the
# window's left and bottom edges.
$LookProbeLeft = 20
$LookProbeBottom = 80
# Brightness (0-255) between the dark page (about 30) and the light page (about 240).
$LookThreshold = 128
# Milliseconds for the phone app to redraw after its night mode changes.
$PhoneSettleMs = 2500

Add-Type -AssemblyName System.Drawing
Add-Type @'
using System;
using System.Runtime.InteropServices;
public static class Win {
    [StructLayout(LayoutKind.Sequential)] public struct RECT { public int Left, Top, Right, Bottom; }
    [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr hWnd);
    [DllImport("user32.dll")] public static extern bool ShowWindow(IntPtr hWnd, int cmd);
    [DllImport("user32.dll")] public static extern bool MoveWindow(IntPtr hWnd, int x, int y, int w, int h, bool repaint);
    [DllImport("user32.dll")] public static extern bool SetProcessDPIAware();
    [DllImport("dwmapi.dll")] public static extern int DwmGetWindowAttribute(IntPtr hWnd, int attr, out RECT rect, int size);
    [DllImport("user32.dll", CharSet = CharSet.Unicode)]
    public static extern IntPtr SendMessageTimeout(IntPtr hWnd, uint msg, UIntPtr wParam, string lParam, uint flags, uint timeout, out UIntPtr result);
}
'@
# Real pixels, not scaled ones, on a high-DPI screen.
[void][Win]::SetProcessDPIAware()

# The window's frame as seen on screen, without Windows 11's invisible resize border.
function Get-Frame([IntPtr]$hwnd) {
    $DWMWA_EXTENDED_FRAME_BOUNDS = 9
    $rect = New-Object Win+RECT
    [void][Win]::DwmGetWindowAttribute($hwnd, $DWMWA_EXTENDED_FRAME_BOUNDS, [ref]$rect, [Runtime.InteropServices.Marshal]::SizeOf($rect))
    $rect
}

function Get-JobFinderWindow {
    $process = Get-Process jobfinder -ErrorAction SilentlyContinue | Where-Object { $_.MainWindowHandle -ne 0 } | Select-Object -First 1
    if (-not $process) { throw 'JobFinder is not running with a window. Start it, then run this again.' }
    $process.MainWindowHandle
}

# Puts the window in front at the set size, near the top left of the main screen.
function Set-Window([IntPtr]$hwnd) {
    $SW_RESTORE = 9
    [void][Win]::ShowWindow($hwnd, $SW_RESTORE)
    [void][Win]::MoveWindow($hwnd, 40, 40, $WindowWidth, $WindowHeight, $true)
    [void][Win]::SetForegroundWindow($hwnd)
    Start-Sleep -Milliseconds $SettleMs
}

$PersonalizeKey = 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Themes\Personalize'

# Windows' app colour mode: light or dark. JobFinder follows it.
function Get-Mode { if ((Get-ItemProperty $PersonalizeKey).AppsUseLightTheme -eq 0) { 'dark' } else { 'light' } }

# Tells running programs the colours changed, as the Settings app does.
function Send-ColorChange {
    $HWND_BROADCAST = [IntPtr]0xffff; $WM_SETTINGCHANGE = 0x1A; $SMTO_ABORTIFHUNG = 2
    $result = [UIntPtr]::Zero
    [void][Win]::SendMessageTimeout($HWND_BROADCAST, $WM_SETTINGCHANGE, [UIntPtr]::Zero, 'ImmersiveColorSet', $SMTO_ABORTIFHUNG, 1000, [ref]$result)
}

function Set-Mode([string]$mode) {
    Set-ItemProperty $PersonalizeKey AppsUseLightTheme ([int]($mode -eq 'light'))
    Send-ColorChange
    Start-Sleep -Milliseconds $SettleMs
}

# Light or dark, as the window shows it now: the page colour in the sidebar.
function Get-ShownLook([IntPtr]$hwnd) {
    $frame = Get-Frame $hwnd
    $bitmap = New-Object System.Drawing.Bitmap 1, 1
    $graphics = [System.Drawing.Graphics]::FromImage($bitmap)
    $graphics.CopyFromScreen($frame.Left + $LookProbeLeft, $frame.Bottom - $LookProbeBottom, 0, 0, $bitmap.Size)
    $brightness = $bitmap.GetPixel(0, 0).GetBrightness() * 255
    $graphics.Dispose(); $bitmap.Dispose()
    if ($brightness -lt $LookThreshold) { 'dark' } else { 'light' }
}

# Waits until JobFinder has redrawn in the look; it follows Windows a moment after the switch.
function Wait-Look([IntPtr]$hwnd, [string]$mode) {
    $until = (Get-Date).AddSeconds($LookTimeoutSeconds)
    $nudged = Get-Date
    while ((Get-ShownLook $hwnd) -ne $mode) {
        if ((Get-Date) -gt $until) { throw "JobFinder didn't turn $mode within $LookTimeoutSeconds s, so no $mode shot was saved." }
        # Windows can answer the first notice with the old mode; say it again until the app follows.
        if (((Get-Date) - $nudged).TotalMilliseconds -ge $NudgeMs) { Send-ColorChange; $nudged = Get-Date }
        Start-Sleep -Milliseconds $LookPollMs
    }
    # One more beat so every part has redrawn, not just the page.
    Start-Sleep -Milliseconds $SettleMs
}

function Save-Shot([IntPtr]$hwnd, [string]$path) {
    $frame = Get-Frame $hwnd
    $width = $frame.Right - $frame.Left; $height = $frame.Bottom - $frame.Top
    $bitmap = New-Object System.Drawing.Bitmap $width, $height
    $graphics = [System.Drawing.Graphics]::FromImage($bitmap)
    $graphics.CopyFromScreen($frame.Left, $frame.Top, 0, 0, $bitmap.Size)
    $bitmap.Save($path, [System.Drawing.Imaging.ImageFormat]::Png)
    $graphics.Dispose(); $bitmap.Dispose()
    Write-Host "Saved $path ($width x $height)"
}

New-Item -ItemType Directory -Force -Path $Out | Out-Null
$Out = (Resolve-Path $Out).Path

function Assert-Tool([string]$tool, [string]$how) {
    if (-not (Get-Command $tool -ErrorAction SilentlyContinue)) { throw "This needs $tool`: $how, then open a new terminal." }
}
$FfmpegHow = 'winget install Gyan.FFmpeg'

if ($What -like 'phone-*') {
    Assert-Tool adb 'winget install Google.PlatformTools, and turn on USB debugging on the phone'
    # Where the phone keeps the file until it is copied over.
    $OnPhone = '/sdcard/jobfinder-capture'
    # Runs adb and stops on failure (no phone, not authorised, more than one phone).
    function Invoke-Adb {
        $output = & adb @args
        if ($LASTEXITCODE -ne 0) { throw "adb $($args -join ' ') failed with code $LASTEXITCODE. Is one phone plugged in with USB debugging allowed?" }
        $output
    }
    # The phone's night mode setting as it is (yes, no, auto, custom_schedule, ...), to put back.
    function Get-PhoneSetting { ((Invoke-Adb shell cmd uimode night) -join ' ') -replace '^.*:\s*', '' }
    function Get-PhoneMode { if ((Get-PhoneSetting) -eq 'yes') { 'dark' } else { 'light' } }
    function Set-PhoneSetting([string]$setting) {
        Invoke-Adb shell cmd uimode night $setting | Out-Null
        Start-Sleep -Milliseconds $PhoneSettleMs
    }
    if ($What -eq 'phone-shot') {
        $was = Get-PhoneSetting
        $modes = if ($Theme -eq 'both') { @('light', 'dark') } else { @($Theme) }
        try {
            foreach ($mode in $modes) {
                Set-PhoneSetting $(if ($mode -eq 'dark') { 'yes' } else { 'no' })
                $path = Join-Path $Out "phone-$Name-$mode.png"
                Invoke-Adb shell screencap -p "$OnPhone.png" | Out-Null
                Invoke-Adb pull "$OnPhone.png" $path | Out-Null
                Invoke-Adb shell rm "$OnPhone.png" | Out-Null
                Write-Host "Saved $path"
            }
        } finally {
            if ((Get-PhoneSetting) -ne $was) { Set-PhoneSetting $was }
        }
        return
    }
    Assert-Tool ffmpeg $FfmpegHow
    $mp4 = Join-Path $env:TEMP "phone-$Name.mp4"
    $path = Join-Path $Out "phone-$Name-$(Get-PhoneMode).webm"
    for ($left = $CountdownSeconds; $left -gt 0; $left--) { Write-Host "Recording in $left..."; Start-Sleep 1 }
    Write-Host "Recording $Seconds s. Go."
    Invoke-Adb shell screenrecord --time-limit $Seconds "$OnPhone.mp4" | Out-Null
    Invoke-Adb pull "$OnPhone.mp4" $mp4 | Out-Null
    Invoke-Adb shell rm "$OnPhone.mp4" | Out-Null
    & ffmpeg -hide_banner -loglevel error -y -i $mp4 -r $ClipFps -c:v libvpx-vp9 -crf $ClipCrf -b:v 0 -row-mt 1 -pix_fmt yuv420p -an $path
    if ($LASTEXITCODE -ne 0) { throw "ffmpeg stopped with code $LASTEXITCODE." }
    Remove-Item $mp4
    Write-Host "Saved $path"
    return
}

$hwnd = Get-JobFinderWindow
Set-Window $hwnd

if ($What -eq 'shot') {
    $was = Get-Mode
    $modes = if ($Theme -eq 'both') { @('light', 'dark') } else { @($Theme) }
    try {
        foreach ($mode in $modes) {
            if ((Get-Mode) -ne $mode) { Set-Mode $mode }
            [void][Win]::SetForegroundWindow($hwnd)
            Wait-Look $hwnd $mode
            Save-Shot $hwnd (Join-Path $Out "$Name-$mode.png")
        }
    } finally {
        if ((Get-Mode) -ne $was) { Set-Mode $was }
    }
    return
}

Assert-Tool ffmpeg $FfmpegHow
$frame = Get-Frame $hwnd
# VP9 wants even sizes.
$width = ($frame.Right - $frame.Left) -band -2; $height = ($frame.Bottom - $frame.Top) -band -2
$path = Join-Path $Out "$Name-$(Get-Mode).webm"
for ($left = $CountdownSeconds; $left -gt 0; $left--) { Write-Host "Recording in $left..."; Start-Sleep 1 }
Write-Host "Recording $Seconds s. Go."
& ffmpeg -hide_banner -loglevel error -y -f gdigrab -framerate $ClipFps `
    -offset_x $frame.Left -offset_y $frame.Top -video_size "${width}x${height}" -draw_mouse 1 -i desktop `
    -t $Seconds -c:v libvpx-vp9 -crf $ClipCrf -b:v 0 -row-mt 1 -pix_fmt yuv420p -an $path
if ($LASTEXITCODE -ne 0) { throw "ffmpeg stopped with code $LASTEXITCODE." }
Write-Host "Saved $path"
