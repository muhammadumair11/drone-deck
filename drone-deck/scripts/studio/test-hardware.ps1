$ErrorActionPreference='Stop'
Add-Type -Path (Join-Path $PSScriptRoot 'mavlink.cs')
function Packet([byte]$Message,[byte[]]$Payload,[byte]$Extra){
 $bytes=New-Object byte[] ($Payload.Length+8);$bytes[0]=254;$bytes[1]=$Payload.Length;$bytes[2]=0;$bytes[3]=1;$bytes[4]=1;$bytes[5]=$Message
 [Array]::Copy($Payload,0,$bytes,6,$Payload.Length);$crc=[uint16]65535
 for($index=1;$index -lt $bytes.Length-2;$index++){$crc=[DroneMavlink]::Accumulate($crc,$bytes[$index])};$crc=[DroneMavlink]::Accumulate($crc,$Extra)
 $bytes[$bytes.Length-2]=[byte]($crc -band 255);$bytes[$bytes.Length-1]=[byte]($crc -shr 8);return ,$bytes
}
$script:phase='startup'
function Read-Event($Process){$task=$Process.StandardOutput.ReadLineAsync();if(-not $task.Wait(12000)){throw ('Hardware fixture response timed out during '+$script:phase)};$line=$task.Result;if(-not $line -or -not $line.StartsWith('@hardware ')){throw ('Unexpected hardware response: '+$line+' '+$Process.StandardError.ReadToEnd())};return ($line.Substring(10)|ConvertFrom-Json)}
$root=Join-Path $PSScriptRoot ('hardware-fixture-'+[guid]::NewGuid().ToString('N'));New-Item -ItemType Directory -Path $root|Out-Null
$reservation=New-Object Net.Sockets.UdpClient(0);$port=$reservation.Client.LocalEndPoint.Port;$reservation.Dispose()
$sender=New-Object Net.Sockets.UdpClient(0);$sender.Client.ReceiveTimeout=5000
$info=New-Object Diagnostics.ProcessStartInfo
$info.FileName=Join-Path $env:WINDIR 'System32\WindowsPowerShell\v1.0\powershell.exe'
$info.Arguments='-NoProfile -ExecutionPolicy Bypass -File "'+(Join-Path $PSScriptRoot 'hardware.ps1')+'" -Endpoint udp:127.0.0.1:'+$port+' -Root "'+$root+'"'
$info.UseShellExecute=$false;$info.CreateNoWindow=$true;$info.RedirectStandardInput=$true;$info.RedirectStandardOutput=$true;$info.RedirectStandardError=$true
$process=[Diagnostics.Process]::Start($info)
try{
 $status=Read-Event $process;if($status.kind -ne 'status'){throw $status.message}
 $script:phase='heartbeat';Write-Output 'Fixture: native worker started'
 $heartbeat=Packet 0 ([byte[]](0,0,0,0,2,12,0,4,3)) 50
 $null=$sender.Send($heartbeat,$heartbeat.Length,'127.0.0.1',$port)
 $vehicle=Read-Event $process
 Write-Output 'Fixture: heartbeat received';$script:phase='parameter request'
 if($vehicle.system -ne 1 -or $vehicle.fields.autopilot -ne 12){throw 'Autopilot identity was not decoded'}
 if($sender.Available -ne 0){throw 'Monitor transmitted before an explicit read request'}
 $process.StandardInput.WriteLine('{"action":"parameters","system":1,"component":1}');$process.StandardInput.Flush()
 $remote=New-Object Net.IPEndPoint([Net.IPAddress]::Any,0);$request=$sender.Receive([ref]$remote)
 if($request[5] -ne 21 -or $request[6] -ne 1 -or $request[7] -ne 1){throw 'Monitor sent an unexpected command'}
 $payload=New-Object byte[] 25;[Array]::Copy([BitConverter]::GetBytes([single]42.25),0,$payload,0,4);$payload[4]=1;[Array]::Copy([Text.Encoding]::ASCII.GetBytes('TEST_PARAM'),0,$payload,8,10);$payload[24]=9
 $parameter=Packet 22 $payload 220;$null=$sender.Send($parameter,$parameter.Length,'127.0.0.1',$port)
 $script:phase='parameter responses'
 $null=Read-Event $process;$null=Read-Event $process;$null=Read-Event $process
 $process.StandardInput.WriteLine('{"action":"stop"}');$process.StandardInput.Flush()
 if(-not $process.WaitForExit(8000)){throw 'Hardware monitor did not stop gracefully'}
 if($process.ExitCode -ne 0){throw $process.StandardError.ReadToEnd()}
 $capture=Get-ChildItem -LiteralPath (Join-Path $root 'hardware') -Directory|Select-Object -First 1
 $backup=Get-Content -Raw -LiteralPath (Join-Path $capture.FullName 'parameters.json')|ConvertFrom-Json
 if(-not $backup.complete -or $backup.parameters.TEST_PARAM.wire_float -ne 42.25){throw 'Parameter snapshot did not preserve the read value'}
 if((Get-Item -LiteralPath (Join-Path $capture.FullName 'capture.tlog')).Length -eq 0){throw 'Telemetry log was empty'}
 'PASS: native UDP identity, passive default, explicit read-only parameter request, parameter backup, tlog and graceful shutdown'
}finally{
 if(-not $process.HasExited){$process.Kill();$process.WaitForExit()};$process.Dispose();$sender.Dispose()
 $resolved=[IO.Path]::GetFullPath($root);$parent=[IO.Path]::GetFullPath($PSScriptRoot).TrimEnd('\')+'\';if(-not $resolved.StartsWith($parent,[StringComparison]::OrdinalIgnoreCase)){throw 'Fixture escaped test directory'}
 Remove-Item -LiteralPath $resolved -Recurse -Force
}
