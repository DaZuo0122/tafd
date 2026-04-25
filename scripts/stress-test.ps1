# TAFD End-to-End Stress Test
# Simulates 600 CPM (10 CPS) sustained typing for 60 seconds while monitoring RSS & CPU.
# Usage: .\scripts\stress-test.ps1
# Requires: PowerShell 5.1+, TAFD built in target\release\

param(
    [int]$Cps = 10,          # Characters per second
    [int]$DurationSec = 60,  # Test duration
    [int]$SampleInterval = 5 # Seconds between measurements
)

$ErrorActionPreference = "Stop"

# Build release binary if missing
$exe = "..\target\release\tafd.exe"
if (-not (Test-Path $exe)) {
    Write-Host "Building release binary..."
    Set-Location ..
    cargo build --release
    Set-Location scripts
}

# Start TAFD
Write-Host "Starting TAFD..."
$proc = Start-Process -FilePath $exe -ArgumentList "--verbose" -PassThru -RedirectStandardOutput "tafd-out.log" -RedirectStandardError "tafd-err.log"
Start-Sleep -Seconds 2

if ($proc.HasExited) {
    Write-Error "TAFD exited immediately. Check tafd-err.log"
    exit 1
}

# C# SendInput wrapper
Add-Type @"
using System;
using System.Runtime.InteropServices;
public class SendInputWrapper {
    [StructLayout(LayoutKind.Sequential)]
    struct INPUT {
        public uint type;
        public KEYBDINPUT ki;
    }
    [StructLayout(LayoutKind.Sequential)]
    struct KEYBDINPUT {
        public ushort wVk;
        public ushort wScan;
        public uint dwFlags;
        public uint time;
        public IntPtr dwExtraInfo;
    }
    [DllImport("user32.dll")] static extern uint SendInput(uint nInputs, [MarshalAs(UnmanagedType.LPArray)] INPUT[] pInputs, int cbSize);

    const uint INPUT_KEYBOARD = 1;
    const uint KEYEVENTF_KEYUP = 0x0002;

    public static void SendKey(ushort vk) {
        INPUT[] inputs = new INPUT[2];
        inputs[0].type = INPUT_KEYBOARD;
        inputs[0].ki.wVk = vk;
        inputs[1].type = INPUT_KEYBOARD;
        inputs[1].ki.wVk = vk;
        inputs[1].ki.dwFlags = KEYEVENTF_KEYUP;
        SendInput((uint)inputs.Length, inputs, Marshal.SizeOf(typeof(INPUT)));
    }
}
"@

$vkCodes = @(0x41, 0x42, 0x43, 0x44, 0x45, 0x46, 0x47, 0x48) # A-H
$intervalMs = [math]::Round(1000 / $Cps)
$start = Get-Date
$measurements = @()
$nextMeasure = $start.AddSeconds($SampleInterval)

Write-Host "Stressing at ${Cps} CPS for ${DurationSec}s..."

while ((Get-Date) - $start).TotalSeconds -lt $DurationSec {
    $vk = $vkCodes | Get-Random
    [SendInputWrapper]::SendKey($vk)

    if ((Get-Date) -ge $nextMeasure) {
        $p = Get-Process -Id $proc.Id -ErrorAction SilentlyContinue
        if ($p) {
            $rssMb = [math]::Round($p.WorkingSet64 / 1MB, 2)
            $cpu = $p.TotalProcessorTime.TotalSeconds
            $measurements += [PSCustomObject]@{
                Time = [math]::Round((($nextMeasure - $start).TotalSeconds), 0)
                RSS_MB = $rssMb
                CPU_Sec = [math]::Round($cpu, 2)
            }
            Write-Host ("  t={0,3}s  RSS={1,6} MB  CPU={2,6}s" -f $measurements[-1].Time, $rssMb, [math]::Round($cpu, 2))
        }
        $nextMeasure = $nextMeasure.AddSeconds($SampleInterval)
    }

    Start-Sleep -Milliseconds $intervalMs
}

# Teardown
Write-Host "Stopping TAFD..."
Stop-Process -Id $proc.Id -Force -ErrorAction SilentlyContinue

# Summary
Write-Host "`n=== Stress Test Summary ==="
if ($measurements.Count -gt 0) {
    $maxRss = ($measurements | Measure-Object -Property RSS_MB -Maximum).Maximum
    $minRss = ($measurements | Measure-Object -Property RSS_MB -Minimum).Minimum
    $avgRss = ($measurements | Measure-Object -Property RSS_MB -Average).Average
    $finalCpu = $measurements[-1].CPU_Sec

    Write-Host ("Max RSS:  {0,6} MB  (target < 10 MB)" -f $maxRss)
    Write-Host ("Min RSS:  {0,6} MB" -f $minRss)
    Write-Host ("Avg RSS:  {0,6} MB" -f ([math]::Round($avgRss, 2)))
    Write-Host ("Final CPU time: {0} sec" -f $finalCpu)
} else {
    Write-Host "No measurements collected."
}

# Cleanup logs
Remove-Item -Path tafd-out.log, tafd-err.log -ErrorAction SilentlyContinue
