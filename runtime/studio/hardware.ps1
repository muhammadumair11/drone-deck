param([Parameter(Mandatory=$true)][string]$Endpoint,[Parameter(Mandatory=$true)][string]$Root)
$ErrorActionPreference='Stop'
. (Join-Path $PSScriptRoot '..\setup\common.ps1')
Add-Type -Path (Join-Path $PSScriptRoot 'mavlink.cs')
function Emit($Value){Write-Output ('@hardware '+($Value|ConvertTo-Json -Depth 8 -Compress))}
$serial=$null;$udp=$null;$tcp=$null;$stream=$null;$log=$null;$failed=$false
$peers=@{};$heartbeats=@{};$parameters=@{};$target='';$sequence=[byte]0
$parameterExpected=0;$lastParameterWrite=[DateTime]::MinValue
$directory=Join-Path $Root ('hardware\'+[DateTime]::UtcNow.ToString('yyyyMMddTHHmmss')+'-'+[guid]::NewGuid().ToString('N').Substring(0,8))
function Save-Parameters {
 if($parameters.Count -gt 0){Write-JsonFile (Join-Path $directory 'parameters.json') @{target=$target;received=$parameters.Count;expected=$parameterExpected;complete=($parameterExpected -gt 0 -and $parameters.Count -eq $parameterExpected);parameters=$parameters;encoding='Raw MAVLink float bytes and declared MAV_PARAM_TYPE preserved. No parameter writes performed.'}}
}
try{
 New-Item -ItemType Directory -Path $directory -Force | Out-Null
 if($Endpoint -match '^serial:(COM[1-9][0-9]*):([0-9]+)$'){
  $baud=[int]$Matches[2];if($baud -lt 9600 -or $baud -gt 3000000){throw 'Serial baud must be 9600–3000000.'}
  $serial=New-Object IO.Ports.SerialPort($Matches[1],$baud);$serial.DtrEnable=$false;$serial.RtsEnable=$false;$serial.ReadTimeout=100;$serial.Open()
 }elseif($Endpoint -match '^udp:([0-9.]+):([0-9]+)$'){
  $port=[int]$Matches[2];if($port -lt 1024 -or $port -gt 65535){throw 'Choose a UDP port from 1024–65535.'}
  $address=[Net.IPAddress]::Parse($Matches[1]);$udp=New-Object Net.Sockets.UdpClient([Net.IPEndPoint]::new($address,$port))
 }elseif($Endpoint -match '^tcp:([A-Za-z0-9._-]+):([0-9]+)$'){
  $port=[int]$Matches[2];if($port -lt 1 -or $port -gt 65535){throw 'Invalid TCP port.'}
  $tcp=New-Object Net.Sockets.TcpClient;$connect=$tcp.ConnectAsync($Matches[1],$port);if(-not $connect.Wait(5000)){throw 'TCP connection timed out.'};$stream=$tcp.GetStream();$stream.ReadTimeout=100
 }else{throw 'Use serial:COM4:115200, udp:127.0.0.1:14555 or tcp:localhost:5760.'}
 $log=[IO.File]::Create((Join-Path $directory 'capture.tlog'))
 Write-JsonFile (Join-Path $directory 'session.json') @{source='User-selected physical/native connection';endpoint=$Endpoint;created=[DateTime]::UtcNow.ToString('o');format='MAVLink tlog with big-endian UTC microsecond timestamps';authenticated_signatures=$false}
 $parser=New-Object DroneMavlink;$inputReader=New-Object DroneInput
 Emit @{kind='status';message='Monitoring only. No heartbeat, arm, mode, parameter-write or flight command is sent automatically.';path=$directory}
 while($true){
  $line=$inputReader.Next()
  if($null -eq $line -and $inputReader.Closed){break}
  if($null -ne $line){
   try{
    $command=$line|ConvertFrom-Json
    if($command.action -eq 'stop'){break}
    if($command.action -eq 'parameters'){
     $target=([int]$command.system).ToString()+':'+([int]$command.component).ToString()
     if(-not $heartbeats.ContainsKey($target) -or ([DateTime]::UtcNow-$heartbeats[$target].seen).TotalSeconds -gt 10){throw 'Choose a vehicle with a fresh verified heartbeat before requesting parameters.'}
     if($heartbeats[$target].signed){throw 'This link uses signing. Use a ground station configured with the correct signing key for parameter requests.'}
     $packet=[DroneMavlink]::ParameterRequest([byte]$command.system,[byte]$command.component,$sequence);$sequence=[byte](($sequence+1)%256)
     if($serial){$serial.Write($packet,0,$packet.Length)}elseif($udp){$null=$udp.Send($packet,$packet.Length,$peers[$target])}else{$stream.Write($packet,0,$packet.Length)}
     $parameters=@{};$parameterExpected=0;Emit @{kind='status';message=('Read-only parameter-list request sent to '+$target)}
    }
   }catch{Emit @{kind='error';message=$_.Exception.Message}}
  }
  if($tcp -and $tcp.Client.Poll(0,[Net.Sockets.SelectMode]::SelectRead) -and $tcp.Available -eq 0){break}
  $bytes=$null;$peer=$null
  if($serial -and $serial.BytesToRead -gt 0){$buffer=New-Object byte[] ([math]::Min($serial.BytesToRead,65535));$count=$serial.Read($buffer,0,$buffer.Length);$bytes=$buffer[0..($count-1)]}
  elseif($udp -and $udp.Available -gt 0){$peer=New-Object Net.IPEndPoint([Net.IPAddress]::Any,0);$bytes=$udp.Receive([ref]$peer)}
  elseif($stream -and $stream.DataAvailable){$buffer=New-Object byte[] 65535;$count=$stream.Read($buffer,0,$buffer.Length);if($count -eq 0){break};$bytes=$buffer[0..($count-1)]}
  if($bytes){foreach($frame in $parser.Push([byte[]]$bytes)){
   $timestamp=[long](([DateTime]::UtcNow-[DateTime]'1970-01-01').TotalMilliseconds*1000);$stamp=[BitConverter]::GetBytes($timestamp);[Array]::Reverse($stamp);$log.Write($stamp,0,8);$log.Write($frame.Bytes,0,$frame.Bytes.Length);$log.Flush()
   if(-not $frame.Verified){continue}
   $key=$frame.System.ToString()+':'+$frame.Component.ToString()
   if($frame.Message -eq 0 -and $frame.Fields['type'] -ne 6 -and $frame.Fields['autopilot'] -ne 8){$heartbeats[$key]=@{seen=[DateTime]::UtcNow;signed=$frame.Signed};if($peer){$peers[$key]=$peer}}
   Emit @{kind='packet';system=$frame.System;component=$frame.Component;message_id=$frame.Message;signed=$frame.Signed;fields=$frame.Fields;path=$directory}
   if($frame.Message -eq 22 -and $target -eq $key){
    $parameters[[string]$frame.Fields['parameter']]=$frame.Fields
    $parameterExpected=[int]$frame.Fields['parameter_count']
    if(([DateTime]::UtcNow-$lastParameterWrite).TotalSeconds -ge 1 -or $parameters.Count -eq $parameterExpected){Save-Parameters;$lastParameterWrite=[DateTime]::UtcNow;Emit @{kind='status';message=('Parameters received '+$parameters.Count+'/'+$parameterExpected+'. Raw values and types saved locally.')}}
   }
  }}else{Start-Sleep -Milliseconds 10}
 }
}catch{$failed=$true;$lastError=$_.Exception.Message;Emit @{kind='error';message=$lastError}}
finally{Save-Parameters;if($log){$log.Dispose()};if($serial){$serial.Dispose()};if($udp){$udp.Dispose()};if($stream){$stream.Dispose()};if($tcp){$tcp.Dispose()};Emit @{kind='status';message=$(if($failed){'Monitoring stopped after error: '+$lastError}else{'Monitoring stopped. Local capture and parameter snapshots retained.'});path=$directory}}
if($failed){exit 1}
