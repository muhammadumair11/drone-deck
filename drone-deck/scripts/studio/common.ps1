. (Join-Path $PSScriptRoot '..\setup\common.ps1')
if(-not ('DroneCredentials' -as [type])) {Add-Type -Path (Join-Path $PSScriptRoot 'credentials.cs')}
if(-not ('DroneHub' -as [type])) {Add-Type -Path (Join-Path $PSScriptRoot 'hub.cs') -IgnoreWarnings}
$script:credentialTarget='DroneDeck/HuggingFace'
function Model-Identity([string]$Repository,[string]$Revision) {
 if($Repository.Length -gt 160 -or $Repository -notmatch '^[A-Za-z0-9][A-Za-z0-9._-]*/[A-Za-z0-9][A-Za-z0-9._-]*$'){throw 'Use a Hugging Face repository identifier such as organization/model.'}
 if(-not $Revision -or $Revision.Length -gt 200 -or $Revision -match '[\s?#\\]'){throw 'Enter a revision name or commit SHA.'}
}
function Model-Path([string]$Root,[string]$Name) {
 if(-not $Name -or $Name -match '[\\:*?"<>|]' -or $Name.StartsWith('/') -or $Name.Split('/') -contains '' -or $Name.Split('/') -contains '..' -or $Name.Split('/') -contains '.' -or $Name -match '(^|/)(CON|PRN|AUX|NUL|COM[1-9]|LPT[1-9])(\.|/|$)' -or $Name -match '[. ](/|$)'){throw 'Repository contains an unsupported Windows file path.'}
 $path=[IO.Path]::GetFullPath((Join-Path $Root $Name.Replace('/','\')))
 $prefix=[IO.Path]::GetFullPath($Root).TrimEnd('\')+'\'
 if(-not $path.StartsWith($prefix,[StringComparison]::OrdinalIgnoreCase)){throw 'File must stay inside its model cache.'}
 return $path
}
function Inspect-Model($Request) {
 Model-Identity $Request.repo $Request.revision
 $url='https://huggingface.co/api/models/'+$Request.repo+'/revision/'+[Uri]::EscapeDataString($Request.revision)+'?blobs=true'
 $metadata=[DroneHub]::Json($url,[DroneCredentials]::Read($script:credentialTarget)) | ConvertFrom-Json
 if($metadata.sha -notmatch '^[a-f0-9]{40}$'){throw 'The repository did not resolve to an immutable commit.'}
 $patterns=@([string]$Request.patterns -split ',' | ForEach-Object {$_.Trim()} | Where-Object {$_})
 if(-not $patterns){throw 'Choose at least one file pattern.'}
 $files=@();$names=@{};$size=[long]0
 foreach($file in $metadata.siblings){
  if(-not @($patterns | Where-Object {$file.rfilename -like $_})){continue}
  $name=[string]$file.rfilename;$null=Model-Path 'C:\DroneDeckModelValidation' $name
  if($names.ContainsKey($name.ToLowerInvariant())){throw 'Repository contains colliding Windows filenames.'};$names[$name.ToLowerInvariant()]=$true
  $hash=[string]$file.lfs.sha256;$blob=[string]$file.blobId
  if($null -eq $file.size -or ($hash -notmatch '^[a-f0-9]{64}$' -and $blob -notmatch '^[a-f0-9]{40}$')){throw ('No verifiable size/hash for '+$name)}
  $size += [long]$file.size;$files+=@{name=$name;size=[long]$file.size;sha256=$hash;blob=$blob}
 }
 if($files.Count -eq 0 -or $files.Count -gt 10000){throw 'Selection must contain between 1 and 10000 files.'}
 return @{repo=$Request.repo;sha=$metadata.sha;license=[string]$metadata.cardData.license;gated=$metadata.gated;files=$files;bytes=$size;created=[DateTime]::UtcNow.ToString('o')}
}
