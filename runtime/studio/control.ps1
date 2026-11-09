param([Parameter(Mandatory=$true)][string]$Root)
. (Join-Path $PSScriptRoot 'common.ps1')
$dataRoot=Join-Path $env:LOCALAPPDATA 'DroneDeck'
$jobsRoot=Join-Path $Root 'studio\jobs'
function Model-JobAlive($State){try{if(-not $State.pid){return $State.status -eq 'Queued' -and ([DateTime]::UtcNow-[DateTime]::Parse($State.updated)).TotalSeconds -lt 20};$process=Get-Process -Id ([int]$State.pid) -ErrorAction Stop;return $process.StartTime.ToFileTimeUtc() -eq [long]$State.process_ticks}catch{return $false}}
function Start-ModelJob($Model) {
 Model-Identity $Model.repo $Model.sha
 if($Model.sha -notmatch '^[a-f0-9]{40}$'){throw 'An immutable reviewed revision is required.'}
 $destination=Join-Path (Join-Path (Join-Path $dataRoot 'models') $Model.repo.Replace('/','--')) $Model.sha
 $drive=[IO.DriveInfo]::new([IO.Path]::GetPathRoot($destination))
 if($drive.AvailableFreeSpace -lt ([long]$Model.bytes+64MB)){throw 'Not enough space in the Windows model cache.'}
 foreach($file in $Model.files){$null=Model-Path $destination $file.name}
 $id=[guid]::NewGuid().ToString('N');$folder=Join-Path $jobsRoot $id
 New-Item -ItemType Directory -Path $folder -Force | Out-Null
 Write-JsonFile (Join-Path $folder 'plan.json') @{id=$id;model=$Model;destination=$destination}
 Write-JsonFile (Join-Path $folder 'state.json') @{id=$id;status='Queued';message='Starting model worker';repo=$Model.repo;path=$destination;pid=0;process_ticks=0;updated=[DateTime]::UtcNow.ToString('o')}
 $arguments='-NoProfile -ExecutionPolicy Bypass -File '+(Quote-Native (Join-Path $PSScriptRoot 'model-download.ps1'))+' -Job '+(Quote-Native $folder)
 Start-Process -FilePath (Join-Path $env:WINDIR 'System32\WindowsPowerShell\v1.0\powershell.exe') -ArgumentList $arguments -WindowStyle Hidden | Out-Null
 return @{id=$id;message='Download started. Progress persists if Drone Deck closes.'}
}
try{
 $request=[Console]::In.ReadToEnd() | ConvertFrom-Json
 switch($request.action){
  'ports' {$data=@{ports=@(Get-CimInstance Win32_SerialPort | Select-Object DeviceID,Name,PNPDeviceID);message='Windows serial devices listed without opening them.'}}
  'credential_status' {$data=@{stored=($null -ne [DroneCredentials]::Read($script:credentialTarget));message='Tokens are stored per Windows user in Credential Manager.'}}
  'save_token' {if($request.token -notmatch '^hf_[A-Za-z0-9]{10,}$'){throw 'Enter a Hugging Face access token.'};[DroneCredentials]::Save($script:credentialTarget,$request.token);$request.token=$null;$data=@{stored=$true;message='Token saved in Windows Credential Manager. Validate it to check current access.'}}
  'forget_token' {[DroneCredentials]::Forget($script:credentialTarget);$data=@{stored=$false;message='Saved Hugging Face token removed.'}}
  'validate_token' {$token=[DroneCredentials]::Read($script:credentialTarget);if(-not $token){throw 'Save a token first.'};$identity=[DroneHub]::Json('https://huggingface.co/api/whoami-v2',$token)|ConvertFrom-Json;$data=@{stored=$true;message=('Authenticated as '+$identity.name)};$token=$null}
  'inspect_model' {
   $model=Inspect-Model $request;$id=[guid]::NewGuid().ToString('N');$previews=Join-Path $Root 'studio\previews';New-Item -ItemType Directory -Path $previews -Force | Out-Null
   Write-JsonFile (Join-Path $previews ($id+'.json')) $model
   $data=@{review=$id;repo=$model.repo;sha=$model.sha;license=$model.license;gated=$model.gated;files=$model.files;bytes=$model.bytes;destination=(Join-Path $dataRoot 'models');message='Review selected files, size, revision and license, then start the download. Model code will not be executed.'}
  }
  'start_model' {
   if($request.review -notmatch '^[a-f0-9]{32}$'){throw 'Inspect and review a model first.'}
   $model=Get-Content -Raw -LiteralPath (Join-Path $Root ('studio\previews\'+$request.review+'.json')) | ConvertFrom-Json
   $data=Start-ModelJob $model
  }
  'retry_model' {
   if($request.id -notmatch '^[a-f0-9]{32}$'){throw 'Select a valid download job.'}
   $previous=Get-Content -Raw -LiteralPath (Join-Path $jobsRoot ($request.id+'\state.json')) | ConvertFrom-Json
   if(Model-JobAlive $previous){throw 'This download is still active. Cancel it or wait before retrying.'}
   $plan=Get-Content -Raw -LiteralPath (Join-Path $jobsRoot ($request.id+'\plan.json')) | ConvertFrom-Json
   $data=Start-ModelJob $plan.model
  }
  'cancel_model' {if($request.id -notmatch '^[a-f0-9]{32}$'){throw 'Select a valid download job.'};$folder=Join-Path $jobsRoot $request.id;if(-not(Test-Path -LiteralPath (Join-Path $folder 'plan.json'))){throw 'Unknown download job.'};$state=Get-Content -Raw -LiteralPath (Join-Path $folder 'state.json')|ConvertFrom-Json;if(-not(Model-JobAlive $state)){throw 'This download is no longer active.'};[IO.File]::WriteAllText((Join-Path $folder 'cancel.request'),'cancel');$data=@{message='Cancellation requested; partial files are retained for retry.'}}
  'models' {
   $models=@();$cache=Join-Path $dataRoot 'models'
   if(Test-Path -LiteralPath $cache){foreach($repository in Get-ChildItem -LiteralPath $cache -Directory){foreach($revision in Get-ChildItem -LiteralPath $repository.FullName -Directory){$manifest=Join-Path $revision.FullName 'drone-deck-model.json';if(Test-Path -LiteralPath $manifest){try{$models+=Get-Content -Raw -LiteralPath $manifest|ConvertFrom-Json}catch{}}}}}
   $data=@{models=$models;message='Local model library loaded without contacting Hugging Face.'}
  }
  'verify_model' {
   Model-Identity $request.repo $request.sha
   if($request.sha -notmatch '^[a-f0-9]{40}$'){throw 'Select a pinned local model revision.'}
   $folder=Join-Path (Join-Path (Join-Path $dataRoot 'models') $request.repo.Replace('/','--')) $request.sha
   $manifest=Get-Content -Raw -LiteralPath (Join-Path $folder 'drone-deck-model.json')|ConvertFrom-Json
   foreach($file in $manifest.files){if(-not[DroneHub]::Verify((Model-Path $folder $file.name),[long]$file.size,[string]$file.sha256,[string]$file.blob)){throw ('Missing or changed model asset: '+$file.name)}}
   $data=@{message='All selected model files verified locally. No network used.';path=$folder}
  }
  default {throw 'Unknown Studio request.'}
 }
 @{ok=$true;action=$request.action;data=$data}|ConvertTo-Json -Depth 30 -Compress
}catch{
 # Never serialize request/token/HTTP headers or exception objects.
 @{ok=$false;action=$request.action;error=$_.Exception.GetBaseException().Message}|ConvertTo-Json -Depth 4 -Compress
}
