$ErrorActionPreference='Stop'
. (Join-Path $PSScriptRoot 'common.ps1')
. (Join-Path $PSScriptRoot 'windows-tools.ps1')
$fixtureDirectory=Join-Path $PSScriptRoot ('windows-fixture-' + [guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path $fixtureDirectory | Out-Null
$fixture=Join-Path $fixtureDirectory 'unrelated.txt'
[IO.File]::WriteAllText($fixture,'Not an executable')
$found=Find-WindowsTool 'qgc' $fixture
if($found.compatible){throw 'Unrelated file was accepted as QGC'}
$real=Find-WindowsTool 'qgc' ''
if($real.found -and -not $real.compatible){throw ('Installed QGC failed identity validation: '+$real.reason)}
$python=(Get-Command python.exe).Source
$timedOut=$false
try { $null=Invoke-Native $python @('-c','import time; time.sleep(4)') 1 (Join-Path $fixtureDirectory 'timeout.log') }catch{$timedOut=$_.Exception.Message -match 'time limit'}
if(-not $timedOut){throw 'Logged process ignored its deadline'}
$null=Invoke-Native $python @('-c','import time; time.sleep(1.5); print("safe transaction finished")') 1 (Join-Path $fixtureDirectory 'preserved.log') -PreserveOnTimeout
if(Test-Path -LiteralPath (Join-Path $fixtureDirectory 'preserved.log.waiting')){throw 'Finished transaction retained a waiting marker'}
$script:installerWaited=$false
$installerFixture=[pscustomobject]@{ExitCode=0}
$installerFixture | Add-Member -MemberType ScriptMethod -Name WaitForExit -Value {$script:installerWaited=$true}
function Cancel-Point {throw 'USER_CANCELLED'}
$cancelled=$false
try {Wait-ReviewedInstaller $installerFixture}catch{$cancelled=$_.Exception.Message -eq 'USER_CANCELLED'}
if(-not $cancelled -or -not $script:installerWaited){throw 'Installer cancellation was not acknowledged after completion'}
Write-Output 'PASS: file identity, actual QGC metadata, logged timeout, safe transaction completion, installer cancellation'
# Delete only the verified generated fixture beneath this test directory.
$resolvedFixture=[IO.Path]::GetFullPath($fixtureDirectory)
$resolvedParent=[IO.Path]::GetFullPath($PSScriptRoot).TrimEnd('\')+'\'
if(-not $resolvedFixture.StartsWith($resolvedParent,[StringComparison]::OrdinalIgnoreCase)){throw 'Fixture escaped test directory'}
Remove-Item -LiteralPath $resolvedFixture -Recurse -Force
