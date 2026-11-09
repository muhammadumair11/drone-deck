function Find-WindowsTool([string]$Tool,[string]$Location) {
    $paths = @()
    $fallback=$null
    if ($Location) { $paths += $Location }
    if (-not $Location -and $Tool -eq 'qgc') {
        $paths += @((Join-Path $env:ProgramFiles 'QGroundControl\bin\QGroundControl.exe'),
                    (Join-Path $env:ProgramFiles 'QGroundControl\QGroundControl.exe'),
                    (Join-Path $env:LOCALAPPDATA 'QGroundControl\bin\QGroundControl.exe'))
    } elseif (-not $Location -and $Tool -eq 'jmavsim') {
        $paths += (Join-Path $env:USERPROFILE 'Desktop\Simulator Launchers\jMAVSim-Windows\jmavsim_run.jar')
        $paths += (Join-Path $env:LOCALAPPDATA 'DroneDeck\tools\jmavsim\jmavsim_run.jar')
    }
    foreach ($path in $paths) {
        if (Test-Path -LiteralPath $path -PathType Leaf) {
            $compatible=$false; $version=''; $runtime=''
            $reason='The selected file is not a verified tool. Choose the actual executable or JAR.'
            if ($Tool -eq 'qgc' -and [IO.Path]::GetExtension($path) -eq '.exe') {
                $info=[Diagnostics.FileVersionInfo]::GetVersionInfo($path)
                $version=[string]$info.ProductVersion
                $stream=[IO.File]::OpenRead($path)
                try { $pe=($stream.ReadByte() -eq 77 -and $stream.ReadByte() -eq 90) } finally {$stream.Dispose()}
                $compatible=$pe -and $info.ProductName -eq 'QGroundControl' -and $version -match '^\d+\.'
                if ($compatible) {$reason='QGroundControl executable and product version verified. Vehicle connection remains a separate check.'}
            } elseif ($Tool -eq 'jmavsim' -and [IO.Path]::GetExtension($path) -eq '.jar') {
                Add-Type -AssemblyName System.IO.Compression.FileSystem
                try {
                    $jar=[IO.Compression.ZipFile]::OpenRead($path)
                    try { $valid=$null -ne $jar.GetEntry('me/drton/jmavsim/Simulator.class') } finally {$jar.Dispose()}
                    $javaCandidates=@(Get-ChildItem -Path (Join-Path $env:ProgramFiles 'Microsoft\jdk-21*\bin\java.exe') -ErrorAction SilentlyContinue | ForEach-Object {$_.FullName})
                    $javaCandidates+=@((Get-Command java.exe -ErrorAction SilentlyContinue).Source)
                    $javaCandidates+=@(Get-ChildItem -Path (Join-Path $env:ProgramFiles 'Microsoft\*\bin\java.exe') -ErrorAction SilentlyContinue | ForEach-Object {$_.FullName})
                    foreach($candidate in @($javaCandidates | Where-Object {$_} | Select-Object -Unique)){
                        $checked=Invoke-Native $candidate @('-version') 15
                        if($checked.code -eq 0 -and ($checked.err+$checked.out) -match 'version "([0-9]+)' -and [int]$Matches[1] -ge 21){$runtime=$candidate;break}
                    }
                    if($valid -and $runtime) {
                        $checked=Invoke-Native $runtime @('-version') 15
                        $compatible=$checked.code -eq 0
                        $version='jMAVSim / ' + ($checked.err -split "`n")[0]
                        $reason='Visualizer JAR and Java runtime verified. Relay/data delivery is checked when the workspace starts.'
                    } elseif($valid) {$reason='jMAVSim JAR found. This native adapter requires Java 21 or newer; install/reuse a compatible runtime.'}
                } catch {$reason='Could not verify the visualizer JAR: ' + $_.Exception.Message}
            }
            $result=@{tool=$Tool;found=$true;compatible=[bool]$compatible;path=$path;executable=$path;version=$version;runtime=$runtime;managed=$false;reason=$reason}
            if($compatible -or $Location){return $result};$fallback=$result
        }
    }
    if($fallback){return $fallback}
    return @{tool=$Tool;found=$false;compatible=$false;path='';executable='';version='';managed=$false;reason='No executable/JAR found. Select its full path manually.'}
}
function Wait-ReviewedInstaller($Installer) {
    $Installer.WaitForExit()
    if ($Installer.ExitCode -notin @(0,3010)) { throw ('Windows installer exited ' + $Installer.ExitCode) }
    Cancel-Point
}
function Install-Qgc([string]$Folder) {
    if (-not [Environment]::Is64BitOperatingSystem -or $env:PROCESSOR_ARCHITECTURE -eq 'ARM64') { throw 'This QGroundControl recipe is for Windows AMD64.' }
    Phase 'Downloading' 'Resolving the official pinned QGroundControl release' 2
    [Net.ServicePointManager]::SecurityProtocol = [Net.SecurityProtocolType]::Tls12
    $release = Invoke-RestMethod -Uri 'https://api.github.com/repos/mavlink/qgroundcontrol/releases/tags/v5.1.5' -Headers @{'User-Agent'='Drone-Deck-Setup'}
    $asset = $release.assets | Where-Object {$_.name -eq 'QGroundControl-installer-AMD64.exe'} | Select-Object -First 1
    if (-not $asset -or $asset.browser_download_url -notlike 'https://github.com/mavlink/qgroundcontrol/releases/download/v5.1.5/*') { throw 'Official release does not contain the expected AMD64 installer.' }
    Add-Type -AssemblyName System.Net.Http
    $destination = Join-Path $Folder $asset.name
    $client = New-Object Net.Http.HttpClient
    $client.DefaultRequestHeaders.UserAgent.ParseAdd('Drone-Deck-Setup')
    try {
        $response = $client.GetAsync($asset.browser_download_url,[Net.Http.HttpCompletionOption]::ResponseHeadersRead).Result
        $response.EnsureSuccessStatusCode() | Out-Null
        $source = $response.Content.ReadAsStreamAsync().Result
        $output = [IO.File]::Create($destination)
        try {
            $buffer = New-Object byte[] 131072
            $downloaded = [long]0
            $lastReport = [DateTime]::UtcNow
            while (($count = $source.Read($buffer,0,$buffer.Length)) -gt 0) {
                Cancel-Point
                $output.Write($buffer,0,$count)
                $downloaded += $count
                if (([DateTime]::UtcNow-$lastReport).TotalSeconds -ge 1) {
                    Phase 'Downloading' ('Downloaded {0:N1} of {1:N1} MiB' -f ($downloaded/1MB),($asset.size/1MB)) 2
                    $lastReport = [DateTime]::UtcNow
                }
            }
        } finally { $output.Dispose(); $source.Dispose(); $response.Dispose() }
    } finally { $client.Dispose() }
    Cancel-Point
    Phase 'Checking' 'Verifying official release checksum or trusted Windows publisher signature' 3
    $digest = [string]$asset.digest
    if ($digest -match '^sha256:([a-fA-F0-9]{64})$') {
        if ((Get-FileHash -LiteralPath $destination -Algorithm SHA256).Hash -ne $Matches[1]) { throw 'Downloaded installer SHA-256 does not match the official release metadata.' }
    } elseif ((Get-AuthenticodeSignature -LiteralPath $destination).Status -ne 'Valid') {
        Phase 'Needs input' 'Downloaded installer has no available published SHA-256 or trusted publisher signature. Inspect the saved package and use official manual setup; it was not executed.' 3
        return
    }
    Phase 'Needs input' 'Complete the visible QGroundControl Windows installer; this job waits for it. Cancellation is acknowledged after it closes.' 4
    $installer = Start-Process -FilePath $destination -PassThru
    Wait-ReviewedInstaller $installer
    $found = Find-WindowsTool 'qgc' ''
    if (-not $found.compatible) { Cancel-Point; Phase 'Needs input' 'Installer finished. Select its installed executable path with Detect if it used a custom folder.' 5; return }
    $script:state.result=$found
    Cancel-Point
    Phase 'Ready' 'QGroundControl executable verified. Apply configuration to use it.' 5
}
