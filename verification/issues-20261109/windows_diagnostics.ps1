$ErrorActionPreference = 'Stop'
$auditDirectory = $PSScriptRoot
. 'C:\Users\HP\Desktop\DroneProjects\drone-control-center\runtime\setup\common.ps1'
. 'C:\Users\HP\Desktop\DroneProjects\drone-control-center\runtime\setup\windows-tools.ps1'
$fixturePath = Join-Path $auditDirectory 'unrelated-file.txt'
[IO.File]::WriteAllText($fixturePath, 'This is not a QGroundControl executable.')
$detected = Find-WindowsTool 'qgc' $fixturePath
$clock = [Diagnostics.Stopwatch]::StartNew()
$result = Invoke-Native (Get-Command python.exe).Source @('-c','import time; time.sleep(2); print("fixture complete")') 1 (Join-Path $auditDirectory 'timeout-fixture.log')
$clock.Stop()
@{
    non_executable_accepted_as_qgc=$detected
    logged_process_timeout=@{timeout_seconds=1;elapsed_seconds=$clock.Elapsed.TotalSeconds;exit_code=$result.code;output=$result.out}
} | ConvertTo-Json -Depth 8
if (-not $detected.compatible -or $clock.Elapsed.TotalSeconds -lt 2 -or $result.code -ne 0) { throw 'Diagnostic observation differed from expectation.' }
