function Install-NativeVisualizer($Candidate,[string]$Distro,[string]$JobFolder) {
 $directory=Join-Path $env:LOCALAPPDATA 'DroneDeck\tools\jmavsim'
 $marker=Join-Path $env:LOCALAPPDATA 'DroneDeck\tools\.owned-jmavsim'
 if((Test-Path -LiteralPath $directory)-and -not(Test-Path -LiteralPath $marker)){throw 'Existing unmanaged native visualizer folder needs manual adoption.'}
 New-Item -ItemType Directory -Path $directory -Force | Out-Null
 [IO.File]::WriteAllText($marker,'Drone Deck managed jMAVSim package from PX4 1.17')
 $linuxZip=$Candidate.path+'/out/production/jMAVSim-distrib.zip'
 $zipPath='\\wsl.localhost\'+$Distro+$linuxZip.Replace('/','\')
 Add-Type -AssemblyName System.IO.Compression.FileSystem
 $archive=[IO.Compression.ZipFile]::OpenRead($zipPath)
 try {
  foreach($entry in $archive.Entries){
   $target=[IO.Path]::GetFullPath((Join-Path $directory $entry.FullName.Replace('/','\')))
   if(-not $target.StartsWith(([IO.Path]::GetFullPath($directory).TrimEnd('\')+'\'),[StringComparison]::OrdinalIgnoreCase)){throw 'Visualizer archive contains an unsafe path.'}
   if($entry.Length -gt 200MB){throw 'Unexpected visualizer archive entry size.'}
   if($entry.FullName.EndsWith('/')){New-Item -ItemType Directory -Path $target -Force | Out-Null;continue}
   New-Item -ItemType Directory -Path (Split-Path $target -Parent) -Force | Out-Null
   $input=$entry.Open();$output=[IO.File]::Create($target)
   try{$input.CopyTo($output)}finally{$input.Dispose();$output.Dispose()}
  }
 }finally{$archive.Dispose()}
 $java=Get-ChildItem -Path (Join-Path $env:ProgramFiles 'Microsoft\jdk-21*\bin\java.exe') -ErrorAction SilentlyContinue | Select-Object -First 1
 if(-not $java){
  $winget=Get-Command winget.exe -ErrorAction SilentlyContinue
  if(-not $winget){throw 'Native package built. Install Microsoft OpenJDK 21 using the official installer, then Detect/reuse this JAR. Windows App Installer/winget was not available.'}
  Cancel-Point
  Phase 'Installing' 'Installing Microsoft OpenJDK 21 through the official winget package; Windows may request approval' 4
  Check-Result (Invoke-Native $winget.Source @('install','--id','Microsoft.OpenJDK.21','--exact','--source','winget','--accept-source-agreements','--accept-package-agreements') 86400 (Join-Path $JobFolder 'output.log') -PreserveOnTimeout)
 }
 Cancel-Point
 . (Join-Path $PSScriptRoot 'windows-tools.ps1')
 $result=Find-WindowsTool 'jmavsim' (Join-Path $directory 'jmavsim_run.jar')
 if(-not $result.compatible){throw $result.reason}
 $result.managed=$true
 return $result
}
