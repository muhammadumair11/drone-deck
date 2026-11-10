param([Parameter(Mandatory=$true)][string]$Root)
. (Join-Path $PSScriptRoot 'common.ps1')

try {
    $request = [Console]::In.ReadToEnd() | ConvertFrom-Json
    $data = $null
    switch ($request.action) {
        'scan' {
            $system = Get-CimInstance Win32_ComputerSystem
            $cpu = Get-CimInstance Win32_Processor | Select-Object -First 1
            $os = Get-CimInstance Win32_OperatingSystem
            $build = [int]$os.BuildNumber
            $virtualization = if ($system.HypervisorPresent -or $cpu.VirtualizationFirmwareEnabled) {'Available'} else {'Not confirmed; check firmware virtualization settings'}
            $distros = @(Get-InstalledDistros)
            $engine = Invoke-Native (Get-Wsl) @('--version') 15
            $data = @{build=$build;windows=$os.Caption;architecture=$os.OSArchitecture;
                eligible=($build -ge 19041 -and [Environment]::Is64BitOperatingSystem);
                virtualization=$virtualization;ram_gib=[math]::Round($system.TotalPhysicalMemory / 1GB,1);
                gpu=(@(Get-CimInstance Win32_VideoController | ForEach-Object {$_.Name}) -join ', ');
                disk_gib=[math]::Round(([IO.DriveInfo]::new([IO.Path]::GetPathRoot($env:LOCALAPPDATA))).AvailableFreeSpace/1GB,1);
                distros=$distros;catalog=@(Get-Recipes);wsl_engine=($engine.code -eq 0 -or $distros.Count -gt 0);wsl_ready=(@($distros | Where-Object {$_.version -eq 2}).Count -gt 0)}
        }
        'online' { $data = @(Get-OnlineDistros) }
        'browse' {
            . (Join-Path $PSScriptRoot 'picker.ps1')
            $data=Select-ToolLocation $request.tool $request.distro
        }
        'detect' {
            if ($request.tool -eq 'qgc') {
                . (Join-Path $PSScriptRoot 'windows-tools.ps1')
                $data = Find-WindowsTool $request.tool $request.location
                break
            }
            if (-not (@(Get-InstalledDistros | Where-Object {$_.name -eq $request.distro -and $_.version -eq 2}))) { throw 'Choose an installed WSL 2 distribution first.' }
            $script = To-LinuxPath (Join-Path $PSScriptRoot 'linux-tools.py')
            $starter = To-LinuxPath (Join-Path $env:USERPROFILE 'Desktop\DroneProjects\px4-mavsdk-starter\.venv')
            $location = if ([string]::IsNullOrWhiteSpace($request.location)) {'@auto'} else {[string]$request.location}
            $result = Invoke-Native (Get-Wsl) @('-d',[string]$request.distro,'--exec','python3',$script,'probe',[string]$request.tool,$location,$starter) 35
            if ($result.code -ne 0) { throw ($result.err + $result.out) }
            $data = $result.out | ConvertFrom-Json
        }
        'initialize' {
            if (-not (@(Get-InstalledDistros | Where-Object {$_.name -eq $request.distro}))) { throw 'Select an installed distribution.' }
            Start-Process -FilePath (Get-Wsl) -ArgumentList @('-d',(Quote-Native $request.distro)) | Out-Null
            $data = @{message='Complete Linux account initialization in the opened terminal, then rescan.'}
        }
        'start' {
            $recipe = Get-Recipes | Where-Object {$_.id -eq $request.tool -and $_.automatic} | Select-Object -First 1
            if (-not $recipe) { throw 'No maintained automatic recipe is available for this tool.' }
            if (Get-Jobs $Root | Where-Object {$_.alive}) { throw 'A setup job is already running. Wait for it to finish to avoid package-manager conflicts.' }
            if ($recipe.target -eq 'WSL') {
                if (-not (@(Get-InstalledDistros | Where-Object {$_.name -eq $request.distro -and $_.version -eq 2}))) { throw 'A working WSL 2 distribution is required.' }
            }
            if ($recipe.id -eq 'distro' -and -not (@(Get-OnlineDistros | Where-Object {$_.name -eq $request.distro}))) { throw 'Choose a distribution from the current Microsoft catalog.' }
            $id = [DateTime]::UtcNow.ToString('yyyyMMddTHHmmss') + '-' + [guid]::NewGuid().ToString('N').Substring(0,8)
            $folder = Join-Path $Root ('setup\jobs\' + $id)
            New-Item -ItemType Directory -Path $folder -Force | Out-Null
            $plan = @{id=$id;tool=$recipe.id;distro=[string]$request.distro;location=[string]$request.location;version=$recipe.version;created=[DateTime]::UtcNow.ToString('o');configure_after=[bool]$request.configure_after}
            Write-JsonFile (Join-Path $folder 'plan.json') $plan
            Write-JsonFile (Join-Path $folder 'state.json') @{id=$id;tool=$recipe.name;status='Queued';message='Waiting for setup worker / Windows approval.';step=0;total=$recipe.steps.Count;updated=[DateTime]::UtcNow.ToString('o');pid=0;started_process=''}
            $arguments = '-NoProfile -ExecutionPolicy Bypass -File ' + (Quote-Native (Join-Path $PSScriptRoot 'run-job.ps1')) + ' -Job ' + (Quote-Native $folder)
            $powershell = Join-Path $env:WINDIR 'System32\WindowsPowerShell\v1.0\powershell.exe'
            try {
                if ($recipe.id -eq 'wsl') { Start-Process -FilePath $powershell -ArgumentList $arguments -Verb RunAs -WindowStyle Hidden | Out-Null }
                else { Start-Process -FilePath $powershell -ArgumentList $arguments -WindowStyle Hidden | Out-Null }
            } catch {
                Write-JsonFile (Join-Path $folder 'state.json') @{id=$id;tool=$recipe.name;status='Needs input';message='Setup did not start; Windows approval may have been declined. Retry when ready.';step=0;total=$recipe.steps.Count;updated=[DateTime]::UtcNow.ToString('o');pid=0;started_process=''}
                throw
            }
            $data = @{id=$id;message='Setup job started. Progress and logs persist across app restarts.'}
        }
        'cancel' {
            if ($request.id -notmatch '^\d{8}T\d{6}-[a-f0-9]{8}$') { throw 'Invalid job identity.' }
            $folder = Join-Path $Root ('setup\jobs\' + $request.id)
            if (-not (Test-Path -LiteralPath (Join-Path $folder 'plan.json'))) { throw 'Unknown setup job.' }
            [IO.File]::WriteAllText((Join-Path $folder 'cancel.request'), 'requested')
            $data = @{message='Cancellation requested. The current package/build step will finish before cancellation.'}
        }
        default { throw 'Unknown setup operation.' }
    }
    @{ok=$true;action=$request.action;data=$data} | ConvertTo-Json -Depth 20 -Compress
} catch {
    @{ok=$false;action=$request.action;error=$_.Exception.Message} | ConvertTo-Json -Depth 5 -Compress
}
