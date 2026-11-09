param([Parameter(Mandatory)][string]$Executable)
$ErrorActionPreference = 'Stop'
Add-Type @'
using System;
using System.Runtime.InteropServices;
public static class IconResourceCheck {
    [DllImport("kernel32.dll", CharSet=CharSet.Unicode, SetLastError=true)]
    public static extern IntPtr LoadLibraryEx(string path, IntPtr file, uint flags);
    [DllImport("kernel32.dll", CharSet=CharSet.Unicode, SetLastError=true)]
    public static extern IntPtr FindResource(IntPtr module, IntPtr name, IntPtr type);
    [DllImport("kernel32.dll")] public static extern IntPtr LoadResource(IntPtr module, IntPtr info);
    [DllImport("kernel32.dll")] public static extern IntPtr LockResource(IntPtr resource);
    [DllImport("kernel32.dll")] public static extern bool FreeLibrary(IntPtr module);
}
'@
$module = [IconResourceCheck]::LoadLibraryEx($Executable, [IntPtr]::Zero, 2)
if ($module -eq [IntPtr]::Zero) { throw 'Could not load executable as a resource file.' }
try {
    $group = [IconResourceCheck]::FindResource($module, [IntPtr]1, [IntPtr]14)
    if ($group -eq [IntPtr]::Zero) { throw 'Executable has no embedded application icon.' }
    $data = [IconResourceCheck]::LockResource([IconResourceCheck]::LoadResource($module, $group))
    $count = [Runtime.InteropServices.Marshal]::ReadInt16($data, 4)
    if ($count -ne 9) { throw "Expected nine icon sizes, found $count." }
    for ($i = 0; $i -lt $count; $i++) {
        $entry = 6 + 14 * $i
        $width = [Runtime.InteropServices.Marshal]::ReadByte($data, $entry)
        if ($width -eq 0) { $width = 256 }
        $id = [Runtime.InteropServices.Marshal]::ReadInt16($data, $entry + 12)
        $frame = [IconResourceCheck]::FindResource($module, [IntPtr]$id, [IntPtr]3)
        if ($frame -eq [IntPtr]::Zero) { throw "Missing $width pixel icon frame." }
        Write-Output "Verified embedded ${width}px icon (resource $id)."
    }
} finally { [IconResourceCheck]::FreeLibrary($module) | Out-Null }
