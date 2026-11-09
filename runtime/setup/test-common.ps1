$ErrorActionPreference = 'Stop'
. (Join-Path $PSScriptRoot 'common.ps1')
# Validate the quoting primitive through a real native argv receiver (Python is test-only).
$echo = Join-Path $env:TEMP ('drone-argv-' + [guid]::NewGuid().ToString('N') + '.py')
[IO.File]::WriteAllText($echo, 'import json,sys; print(json.dumps(sys.argv[1:]))')
try {
    $values = @('path with spaces', 'embedded"quote', 'trailing\', '$(not-a-shell)', "apostrophe's path")
    $process = Invoke-Native 'C:\Python312\python.exe' (@($echo) + $values) 10
    if ($process.code -ne 0) { throw $process.err }
    $actual = ConvertFrom-Json -InputObject $process.out
    if ($actual.Count -ne $values.Count) { throw 'Native argument count changed.' }
    for ($i=0; $i -lt $values.Count; $i++) { if ($actual[$i] -cne $values[$i]) { throw "Native argument roundtrip failed at index $i." } }
} finally { Remove-Item -LiteralPath $echo -Force }
$script:fixture = "  NAME                   STATE           VERSION`n* Ubuntu-24.04            Running         2`n  Debian                 Stopped         1`n"
function Invoke-Native { param($Program,$Arguments,$Timeout,$Log); return @{code=0;out=$script:fixture;err=''} }
$installed = @(Get-InstalledDistros)
if ($installed.Count -ne 2 -or $installed[0].name -ne 'Ubuntu-24.04' -or $installed[1].version -ne 1) { throw 'Installed distribution parsing failed.' }
$script:fixture = "NAME                            FRIENDLY NAME`nUbuntu-24.04                    Ubuntu 24.04 LTS`nDebian                          Debian GNU/Linux`n"
$online = @(Get-OnlineDistros)
if ($online.Count -ne 2 -or $online[0].support -ne 'Recommended') { throw 'Online catalog parsing failed.' }
$script:fixture = 'No valid catalog rows'
try { Get-OnlineDistros | Out-Null; throw 'Expected a catalog error' } catch { if ($_.Exception.Message -eq 'Expected a catalog error') { throw } }
Write-Output 'SETUP_COMMON_TESTS_PASSED: literal native arguments, installed/catalog parsing, unavailable catalog handling.'
