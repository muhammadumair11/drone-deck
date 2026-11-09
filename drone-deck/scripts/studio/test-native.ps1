param([switch]$Network,[switch]$CredentialStore)
$ErrorActionPreference='Stop'
. (Join-Path $PSScriptRoot 'common.ps1')
Add-Type -Path (Join-Path $PSScriptRoot 'mavlink.cs')
$directory=Join-Path $PSScriptRoot ('native-fixture-'+[guid]::NewGuid().ToString('N'));New-Item -ItemType Directory -Path $directory|Out-Null
try{
 if($CredentialStore){$credential='DroneDeck/Test/'+[guid]::NewGuid().ToString('N');try{[DroneCredentials]::Save($credential,'hf_nonsecret_test_fixture_value');if([DroneCredentials]::Read($credential) -ne 'hf_nonsecret_test_fixture_value'){throw 'Credential round trip failed'}}finally{[DroneCredentials]::Forget($credential)};if($null -ne [DroneCredentials]::Read($credential)){throw 'Temporary credential was not removed'};'PASS: isolated Windows Credential Manager round trip and cleanup'}
 foreach($name in @('../outside','C:/outside','CON/file','file.','a//b','a\b')){$rejected=$false;try{$null=Model-Path $directory $name}catch{$rejected=$true};if(-not $rejected){throw ('Unsafe model path accepted: '+$name)}}
 $file=Join-Path $directory 'asset';[IO.File]::WriteAllText($file,'fixture')
 $hash=(Get-FileHash -LiteralPath $file -Algorithm SHA256).Hash.ToLowerInvariant()
 if(-not [DroneHub]::Verify($file,7,$hash,'')){throw 'Valid model hash failed'}
 [IO.File]::WriteAllText($file,'changed');if([DroneHub]::Verify($file,7,$hash,'')){throw 'Changed model hash passed'}
 $parser=New-Object DroneMavlink
 $frame=[DroneMavlink]::ParameterRequest(1,1,0)
 if($parser.Push($frame[0..4]).Count -ne 0){throw 'Incomplete frame accepted'}
 $parsed=$parser.Push($frame[5..9]);if($parsed.Count -ne 1 -or -not $parsed[0].Verified -or $parsed[0].Message -ne 21){throw 'Parameter request framing/CRC failed'}
 $frame[8]=$frame[8] -bxor 1;if($parser.Push($frame)[0].Verified){throw 'Corrupted MAVLink checksum accepted'}
 if($Network){
  $repo='hf-internal-testing/tiny-random-bert'
  $metadata=[DroneHub]::Json(('https://huggingface.co/api/models/'+$repo+'/revision/main?blobs=true'),$null)|ConvertFrom-Json
  $config=$metadata.siblings|Where-Object {$_.rfilename -eq 'config.json'}|Select-Object -First 1
  if(-not $config -or $config.size -gt 10000){throw 'Unexpected public metadata fixture size'}
  $target=Join-Path $directory 'config.json'
  [DroneHub]::Download(('https://huggingface.co/'+$repo+'/resolve/'+$metadata.sha+'/config.json'),$null,$target,[long]$config.size,[string]$config.lfs.sha256,[string]$config.blobId,(Join-Path $directory 'cancel'),(Join-Path $directory 'progress.json'),0,[long]$config.size)
  if(-not [DroneHub]::Verify($target,[long]$config.size,[string]$config.lfs.sha256,[string]$config.blobId)){throw 'Public metadata fixture failed verification'}
  Write-Output ('PASS: public pinned config fixture verified ('+$config.size+' bytes); no weights downloaded')
 }
 Write-Output 'PASS: path boundaries, model integrity, fragmented MAVLink parsing and corrupted CRC rejection'
}finally{
 $resolved=[IO.Path]::GetFullPath($directory);$parent=[IO.Path]::GetFullPath($PSScriptRoot).TrimEnd('\')+'\'
 if(-not $resolved.StartsWith($parent,[StringComparison]::OrdinalIgnoreCase)){throw 'Fixture escaped test directory'}
 Remove-Item -LiteralPath $resolved -Recurse -Force
}
