param([Parameter(Mandatory=$true)][string]$Job)
. (Join-Path $PSScriptRoot 'common.ps1')
$plan = Get-Content -Raw -LiteralPath (Join-Path $Job 'plan.json') | ConvertFrom-Json
$recipe = Get-Recipes | Where-Object {$_.id -eq $plan.tool -and $_.automatic} | Select-Object -First 1
if (-not $recipe) { throw 'Invalid setup recipe.' }
$log = Join-Path $Job 'output.log'
$statePath = Join-Path $Job 'state.json'
$current = Get-Process -Id $PID
$script:state = @{id=$plan.id;tool=$recipe.name;status='Running';message='Setup worker started';step=0;total=$recipe.steps.Count;
    updated=[DateTime]::UtcNow.ToString('o');pid=$PID;started_process=$current.StartTime.ToUniversalTime().ToString('o');process_ticks=$current.StartTime.ToFileTimeUtc();result=$null;configure_after=[bool]$plan.configure_after}
function Phase([string]$Status,[string]$Message,[int]$Step) {
    $script:state.status=$Status; $script:state.message=$Message; $script:state.step=$Step
    $script:state.updated=[DateTime]::UtcNow.ToString('o')
    Write-JsonFile $statePath $script:state
    [IO.File]::AppendAllText($log, ([DateTime]::UtcNow.ToString('o') + ' ' + $Message + "`n"))
}
function Cancel-Point { if (Test-Path -LiteralPath (Join-Path $Job 'cancel.request')) { throw 'USER_CANCELLED' } }
function Check-Result($Result) { if ($Result.code -ne 0) { throw ('Setup command exited ' + $Result.code + ': ' + $Result.err + $Result.out) } }
function Linux([string[]]$Arguments,[switch]$AsRoot) {
    $argsList = @('-d',[string]$plan.distro)
    if ($AsRoot) { $argsList += @('-u','root') }
    $argsList += @('--exec','python3',(To-LinuxPath (Join-Path $PSScriptRoot 'linux-tools.py'))) + $Arguments
    $result = if($Arguments[0] -in @('probe','verify','preflight')) {Invoke-Native (Get-Wsl) $argsList 45 $log}
              else {Invoke-Native (Get-Wsl) $argsList 86400 $log -PreserveOnTimeout}
    Check-Result $result
    return $result
}
function Probe-Linux {
    $starter = To-LinuxPath (Join-Path $env:USERPROFILE 'Desktop\DroneProjects\px4-mavsdk-starter\.venv')
    $location = if ([string]::IsNullOrWhiteSpace($plan.location)) {'@auto'} else {[string]$plan.location}
    (Linux @('probe',[string]$plan.tool,$location,$starter)).out | ConvertFrom-Json
}
$mutex = New-Object Threading.Mutex($false, 'Local\DroneDeck.SetupInstaller')
$locked = $false
try {
    $locked = $mutex.WaitOne(0)
    if (-not $locked) { throw 'Another Drone Deck setup worker is running; retry after it completes.' }
    Phase 'Running' 'Checking existing installation and prerequisites' 1
    Cancel-Point
    switch ($plan.tool) {
        'wsl' {
            if ([int](Get-CimInstance Win32_OperatingSystem).BuildNumber -lt 19041) { throw 'This Windows build does not support the guided WSL installation command.' }
            Phase 'Installing' 'Installing WSL; Windows may require a restart afterward' 2
            $result = Invoke-Native (Get-Wsl) @('--install','--no-distribution') 86400 $log -PreserveOnTimeout
            if ($result.code -notin @(0,3010)) { throw ($result.err + $result.out) }
            $restart = $result.code -eq 3010 -or (Test-Path 'HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\Component Based Servicing\RebootPending')
            if ($restart) { Phase 'Needs restart' 'Restart Windows, reopen Drone Deck, and rescan to continue with a Linux distribution.' 3 }
            else { Phase 'Ready' 'WSL installation command completed. Rescan and install or select a WSL 2 distribution.' 3 }
        }
        'distro' {
            $existing = @(Get-InstalledDistros | Where-Object {$_.name -eq $plan.distro})
            if (-not $existing) {
                Phase 'Downloading' ('Downloading/installing ' + $plan.distro + '; native progress appears in the setup log') 2
                $result = Invoke-Native (Get-Wsl) @('--install','-d',[string]$plan.distro,'--no-launch','--web-download') 86400 $log -PreserveOnTimeout
                if ($result.code -eq 3010) { Phase 'Needs restart' 'Restart Windows and rescan the distribution.' 3; break }
                Check-Result $result
            }
            Cancel-Point
            $installed = @(Get-InstalledDistros | Where-Object {$_.name -eq $plan.distro -and $_.version -eq 2})
            if (-not $installed) { throw 'Distribution was not confirmed as WSL 2. Review the log and Microsoft setup instructions.' }
            Phase 'Needs input' 'Distribution installed. Open Initialize Linux user, complete first-run account setup, then rescan.' 3
        }
        'qgc' {
            . (Join-Path $PSScriptRoot 'windows-tools.ps1')
            $found = Find-WindowsTool 'qgc' ([string]$plan.location)
            if ($found.found -and -not $found.compatible) { throw ('Existing QGroundControl candidate needs review: ' + $found.reason) }
            if ($found.compatible) { $script:state.result=$found; Phase 'Existing installation' 'QGroundControl is already installed. Apply its verified configuration to reuse it.' 5; break }
            Install-Qgc $Job
        }
        default {
            $probeUser = Invoke-Native (Get-Wsl) @('-d',[string]$plan.distro,'--exec','id','-u') 20
            Check-Result $probeUser
            if ($probeUser.out.Trim() -eq '0') { throw 'Initialize a regular Linux user before creating project tools.' }
            $python = Invoke-Native (Get-Wsl) @('-d',[string]$plan.distro,'--exec','/usr/bin/test','-x','/usr/bin/python3') 20
            if ($python.code -ne 0) {
                $noble = Invoke-Native (Get-Wsl) @('-d',[string]$plan.distro,'--exec','/bin/sh','-c','. /etc/os-release; test "$ID:$VERSION_ID" = "ubuntu:24.04"') 20
                Check-Result $noble
                Phase 'Installing' 'Installing the missing Python prerequisite in Ubuntu 24.04' 1
                Check-Result (Invoke-Native (Get-Wsl) @('-d',[string]$plan.distro,'-u','root','--exec','apt-get','update') 86400 $log -PreserveOnTimeout)
                Check-Result (Invoke-Native (Get-Wsl) @('-d',[string]$plan.distro,'-u','root','--exec','env','DEBIAN_FRONTEND=noninteractive','apt-get','install','-y','--no-install-recommends','python3') 86400 $log -PreserveOnTimeout)
            }
            $found = Probe-Linux
            Cancel-Point
            if ($found.found -and $found.compatible) {
                $script:state.result=$found
                Phase 'Existing installation' 'Compatible launch candidate found. Nothing was updated. Detect and apply its configuration to reuse it.' $recipe.steps.Count
                break
            }
            if ($plan.tool -ne 'ros2') { $null = Linux @('preflight',[string]$plan.tool) }
            Phase 'Installing' 'Installing shared Linux prerequisites. Cancellation waits for the package step to finish.' 2
            if ($plan.tool -eq 'ros2') {
                $installer = To-LinuxPath (Join-Path $PSScriptRoot '..\install-ros-recording.py')
                $result = Invoke-Native (Get-Wsl) @('-d',[string]$plan.distro,'-u','root','--exec','python3',$installer) 86400 $log -PreserveOnTimeout
                Check-Result $result
            } else {
                $null = Linux @('dependencies',[string]$plan.tool) -AsRoot
                Cancel-Point
                Phase 'Installing' 'Downloading/building the pinned tool in its app-owned Linux folder' 3
                $null = Linux @('install',[string]$plan.tool)
            }
            Cancel-Point
            Phase 'Checking' 'Verifying the installed tool and version' $recipe.steps.Count
            $result = (Linux @('verify',[string]$plan.tool)).out | ConvertFrom-Json
            if (-not $result.found -or -not $result.compatible) { throw ('Installation completed but verification needs attention: ' + $result.reason) }
            $script:state.result=$result
            Phase 'Ready' 'Verified. Apply configuration to connect this installation to your workspace profiles.' $recipe.steps.Count
        }
    }
} catch {
    if ($_.Exception.Message -eq 'USER_CANCELLED') { Phase 'Cancelled' 'Cancelled at a safe step boundary. Installed dependencies were preserved.' $script:state.step }
    else { Phase 'Failed' $_.Exception.Message $script:state.step }
} finally {
    if ($locked) { $mutex.ReleaseMutex() }
    $mutex.Dispose()
}
