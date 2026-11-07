# Captures Minecraft Dungeons Arcade 3.07's decrypted asset seed tables from a
# running TeknoParrot (ElfLdr2) session on Windows. Read-only: it only calls
# ReadProcessMemory on BudgieLoader_x64.exe and writes minecraft-seeds.json.
#
# Usage (PowerShell, same user as TeknoParrot):
#   1. Start this script:   powershell -ExecutionPolicy Bypass -File minecraft-capture-seeds.ps1
#   2. Launch Minecraft Dungeons Arcade from TeknoParrot.
#   3. Wait for the attract screen. The script stops by itself once the tables change.
param([int]$TimeoutSeconds = 600, [string]$Out = "minecraft-seeds.json")

Add-Type @"
using System; using System.Runtime.InteropServices;
public static class Mem {
  [DllImport("kernel32.dll", SetLastError=true)] public static extern IntPtr OpenProcess(int a, bool b, int pid);
  [DllImport("kernel32.dll", SetLastError=true)] public static extern bool ReadProcessMemory(IntPtr h, IntPtr addr, byte[] buf, IntPtr size, out IntPtr read);
  [DllImport("kernel32.dll")] public static extern IntPtr VirtualQueryEx(IntPtr h, IntPtr addr, out MBI info, IntPtr len);
  [StructLayout(LayoutKind.Sequential)] public struct MBI { public IntPtr BaseAddress, AllocationBase; public uint AllocationProtect; public IntPtr RegionSize; public uint State, Protect, Type; }
}
"@

# Guest virtual addresses in gamedump.exec (SHA256 663b1479...6be3b).
$SEED64   = 0x16d3d40   # 64-byte table, hasp-decrypted in place by 0x64da60
$SEED128  = 0x16d3d80   # 128-byte table, hasp-decrypted in place by 0x64da60
$ANCHOR   = 0x16d41e0   # 64-byte table that is never decrypted (used to find the guest base)
$anchorHex = "46894b102b3d7471bef90afbe683c2e6d38429d356445ee4b22e59cdc2e1b79ed0c897f7dcc05b39352eeda568f039d3"  # first 48 bytes
$cipher64Hex  = "73b25110043c7fbfeec67ed283e1c2ab3935e0956707232a13cfaf0efaf718ed101e828331b8349e61d193520ed18cbc"
$cipher128Hex = "7937348ba3964ea3de7938eb9359935f959f96fe0aeb70031979e73b1124f5c7c362c0caf8e4ecf7c1113f1c451e6f5d"

function Hex([byte[]]$b) { ($b | ForEach-Object { $_.ToString("x2") }) -join "" }
function FromHex([string]$s) { [byte[]]($s -split '(..)' | Where-Object { $_ } | ForEach-Object { [Convert]::ToByte($_, 16) }) }
function Read([IntPtr]$h, [Int64]$addr, [int]$n) {
  $buf = New-Object byte[] $n; $r = [IntPtr]::Zero
  if ([Mem]::ReadProcessMemory($h, [IntPtr]$addr, $buf, [IntPtr]$n, [ref]$r) -and $r.ToInt64() -eq $n) { return ,$buf } return $null
}
function FindAnchor([IntPtr]$h) {
  $needle = FromHex $anchorHex; $addr = [Int64]0; $mbi = New-Object Mem+MBI
  $size = [IntPtr][Runtime.InteropServices.Marshal]::SizeOf($mbi)
  while ([Mem]::VirtualQueryEx($h, [IntPtr]$addr, [ref]$mbi, $size) -ne [IntPtr]::Zero) {
    $len = $mbi.RegionSize.ToInt64()
    if ($mbi.State -eq 0x1000 -and ($mbi.Protect -band 0x66) -and $len -lt 512MB) {
      $data = Read $h $mbi.BaseAddress.ToInt64() ([int]$len)
      if ($data) {
        $i = 0
        while (($i = [Array]::IndexOf($data, $needle[0], $i)) -ge 0 -and $i -le $data.Length - $needle.Length) {
          $ok = $true; for ($k = 1; $k -lt $needle.Length; $k++) { if ($data[$i+$k] -ne $needle[$k]) { $ok = $false; break } }
          if ($ok) { return $mbi.BaseAddress.ToInt64() + $i - $ANCHOR }
          $i++
        }
      }
    }
    $addr = $mbi.BaseAddress.ToInt64() + $len
  }
  return $null
}

$deadline = (Get-Date).AddSeconds($TimeoutSeconds); $base = $null; $h = [IntPtr]::Zero
Write-Host "Waiting for BudgieLoader_x64.exe..."
while ((Get-Date) -lt $deadline) {
  $p = Get-Process -Name BudgieLoader_x64 -ErrorAction SilentlyContinue | Sort-Object WorkingSet64 -Descending | Select-Object -First 1
  if (-not $p) { Start-Sleep -Milliseconds 500; continue }
  if ($h -eq [IntPtr]::Zero) { $h = [Mem]::OpenProcess(0x0410, $false, $p.Id); Write-Host "Opened pid $($p.Id)" }
  if ($null -eq $base) {
    $direct = Read $h $ANCHOR 48
    if ($direct -and (Hex $direct) -eq $anchorHex) { $base = 0 } else { $base = FindAnchor $h }
    if ($null -ne $base) { Write-Host ("Guest base offset: 0x{0:x}" -f $base) } else { Start-Sleep -Seconds 1; continue }
  }
  $s64 = Read $h ($base + $SEED64) 64; $s128 = Read $h ($base + $SEED128) 128
  if ($s64 -and $s128) {
    $h64 = Hex $s64; $h128 = Hex $s128
    if (-not $h64.StartsWith($cipher64Hex) -and -not $h128.StartsWith($cipher128Hex)) {
      $result = [ordered]@{
        game_sha256 = "663b147927e7b9d60cef7bd41cf49d3fd64058619880391083ea1ba872d6be3b"
        guest_base_offset = $base; seed64_va = "0x16d3d40"; seed128_va = "0x16d3d80"
        seed64 = $h64; seed128 = $h128; captured = (Get-Date).ToString("o")
      }
      $result | ConvertTo-Json | Set-Content -Encoding ASCII $Out
      Write-Host "Captured decrypted tables -> $Out"; exit 0
    }
  }
  Start-Sleep -Milliseconds 200
}
Write-Host "Timed out. Tables never changed from their encrypted values (base found: $($null -ne $base))."; exit 1
