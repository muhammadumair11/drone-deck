param([Parameter(Mandatory=$true)][string]$Job)
. (Join-Path $PSScriptRoot 'common.ps1')
$plan=Get-Content -Raw -LiteralPath (Join-Path $Job 'plan.json') | ConvertFrom-Json
$process=Get-Process -Id $PID
$state=@{id=$plan.id;status='Downloading';message='Downloading the reviewed immutable model revision';pid=$PID;process_ticks=$process.StartTime.ToFileTimeUtc();repo=$plan.model.repo;path=$plan.destination;updated=[DateTime]::UtcNow.ToString('o')}
function Status([string]$Status,[string]$Message){$state.status=$Status;$state.message=$Message;$state.updated=[DateTime]::UtcNow.ToString('o');Write-JsonFile (Join-Path $Job 'state.json') $state}
$mutex=New-Object Threading.Mutex($false,('Local\DroneDeck.Model.'+$plan.model.repo.Replace('/','--')+'.'+$plan.model.sha))
$locked=$false
try{
 $locked=$mutex.WaitOne(0);if(-not $locked){throw 'This model revision is already being downloaded by another job.'}
 Status 'Downloading' 'Starting verified model download'
 $token=[DroneCredentials]::Read($script:credentialTarget);$done=[long]0
 foreach($file in $plan.model.files){
  if(Test-Path -LiteralPath (Join-Path $Job 'cancel.request')){throw 'CANCELLED'}
  $target=Model-Path $plan.destination $file.name
  $encoded=($file.name.Split('/') | ForEach-Object {[Uri]::EscapeDataString($_)}) -join '/'
  $url='https://huggingface.co/'+$plan.model.repo+'/resolve/'+$plan.model.sha+'/'+$encoded
  [DroneHub]::Download($url,$token,$target,[long]$file.size,[string]$file.sha256,[string]$file.blob,(Join-Path $Job 'cancel.request'),(Join-Path $Job 'progress.json'),$done,[long]$plan.model.bytes)
  $done += [long]$file.size
 }
 $manifest=@{repo=$plan.model.repo;sha=$plan.model.sha;license=$plan.model.license;files=$plan.model.files;bytes=$plan.model.bytes;path=$plan.destination;verified=[DateTime]::UtcNow.ToString('o');executable_code_enabled=$false}
 Write-JsonFile (Join-Path $plan.destination 'drone-deck-model.json') $manifest
 Status 'Ready' 'All selected files verified. This downloads assets; select an inference runtime separately.'
}catch{
 $cancelled=(Test-Path -LiteralPath (Join-Path $Job 'cancel.request'))
 Status $(if($cancelled){'Cancelled'}else{'Failed'}) $(if($cancelled){'Cancelled. Partial files for this pinned revision are retained for retry.'}else{$_.Exception.GetBaseException().Message})
}finally{$token=$null;if($locked){$mutex.ReleaseMutex()};$mutex.Dispose()}
