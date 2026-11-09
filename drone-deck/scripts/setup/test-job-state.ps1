$ErrorActionPreference = 'Stop'
. (Join-Path $PSScriptRoot 'common.ps1')
foreach ($file in Get-ChildItem -LiteralPath $PSScriptRoot -Filter '*.ps1') {
    $parseErrors=$null; $tokens=$null
    [Management.Automation.Language.Parser]::ParseFile($file.FullName,[ref]$tokens,[ref]$parseErrors) | Out-Null
    if ($parseErrors) { throw ($file.Name + ': ' + ($parseErrors.Message -join '; ')) }
}
$fixtureRoot = Join-Path ([IO.Path]::GetTempPath()) ('drone-setup-fixture-' + [guid]::NewGuid().ToString('N'))
$jobId = '20261109T000000-12345678'
$jobFolder = Join-Path $fixtureRoot ('setup\jobs\' + $jobId)
New-Item -ItemType Directory -Path $jobFolder -Force | Out-Null
$workerFile = Join-Path $fixtureRoot 'non-installing-fixture.ps1'
$commonFile = Join-Path $PSScriptRoot 'common.ps1'
$fixtureCode = @'
param([string]$Common,[string]$Folder)
. $Common
$process = Get-Process -Id $PID
$state = @{id='20261109T000000-12345678';tool='Non-installing test fixture';status='Running';message='Writing fixture status only';step=1;total=2;pid=$PID;process_ticks=$process.StartTime.ToFileTimeUtc();started_process=$process.StartTime.ToUniversalTime().ToString('o');updated=[DateTime]::UtcNow.ToString('o')}
Write-JsonFile (Join-Path $Folder 'state.json') $state
[IO.File]::WriteAllText((Join-Path $Folder 'output.log'),'Fixture log: no installer or package command is executed.')
Start-Sleep -Milliseconds 1200
$state.status='Ready'; $state.step=2; $state.message='Fixture finished'; $state.updated=[DateTime]::UtcNow.ToString('o')
Write-JsonFile (Join-Path $Folder 'state.json') $state
'@
[IO.File]::WriteAllText($workerFile,$fixtureCode)
$shell = Join-Path $env:WINDIR 'System32\WindowsPowerShell\v1.0\powershell.exe'
$argsLine = '-NoProfile -ExecutionPolicy Bypass -File ' + (Quote-Native $workerFile) + ' -Common ' + (Quote-Native $commonFile) + ' -Folder ' + (Quote-Native $jobFolder)
$worker = Start-Process -FilePath $shell -ArgumentList $argsLine -PassThru -WindowStyle Hidden -RedirectStandardOutput (Join-Path $fixtureRoot 'worker.out') -RedirectStandardError (Join-Path $fixtureRoot 'worker.err')
$runningSeen=$false; $readySeen=$false; $deadline=[DateTime]::UtcNow.AddSeconds(15)
do {
    $jobs = @(Get-Jobs $fixtureRoot)
    if ($jobs.Count) {
        $runningSeen = $runningSeen -or ($jobs[0].status -eq 'Running' -and $jobs[0].alive)
        $readySeen = $readySeen -or $jobs[0].status -eq 'Ready'
    }
    if ($readySeen) { break }
    Start-Sleep -Milliseconds 100
} while ([DateTime]::UtcNow -lt $deadline)
$worker.WaitForExit()
if (-not $runningSeen -or -not $readySeen) {
    Get-Content -LiteralPath (Join-Path $fixtureRoot 'worker.err') -ErrorAction SilentlyContinue
    if (Test-Path -LiteralPath (Join-Path $jobFolder 'state.json')) { Get-Content -Raw -LiteralPath (Join-Path $jobFolder 'state.json') }
    throw ('Persistent fixture progress/process identity was not observed. Fixture: ' + $fixtureRoot)
}
$state = Get-Content -Raw -LiteralPath (Join-Path $jobFolder 'state.json') | ConvertFrom-Json
$state.status='Running'; $state.updated=[DateTime]::UtcNow.AddMinutes(-1).ToString('o')
Write-JsonFile (Join-Path $jobFolder 'state.json') $state
$interrupted = @(Get-Jobs $fixtureRoot)
if ($interrupted[0].status -ne 'Interrupted') { throw 'Interrupted worker recovery was not recognized.' }
Write-Output 'SETUP_JOB_FIXTURE_PASSED: atomic status, live process identity, progress, logs, interrupted-worker recovery; no installation entry point was executed.'
