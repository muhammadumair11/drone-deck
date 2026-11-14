function Find-WindowsFolder([string]$Tool,[string]$Location,$Check) {
    # A tool installed by hand into a folder the user chose: every `expect` entry must exist.
    $result=@{tool=$Tool;found=$false;compatible=$false;path='';executable='';version='';runtime='';managed=$false;
        reason='Choose the folder where you installed it, then check again.'}
    if (-not $Location) { return $result }
    if (-not (Test-Path -LiteralPath $Location -PathType Container)) {
        $result.reason='That folder does not exist. Choose the folder where you installed it.'
        return $result
    }
    $missing=@($Check.expect | Where-Object { -not (Test-Path -LiteralPath (Join-Path $Location $_)) })
    $first=Join-Path $Location ([string]@($Check.expect)[0])
    $result.found=$true
    $result.path=$Location
    $result.executable=if (Test-Path -LiteralPath $first -PathType Leaf) {$first} else {''}
    if ($missing.Count -eq 0) {
        $result.compatible=$true
        $result.reason='Every expected file is in place. Whether the simulator talks to the autopilot is checked when the workspace starts.'
    } else {
        $result.reason='Missing in this folder: ' + ($missing -join ', ') + '. Finish the guide steps, then check again.'
    }
    return $result
}
function Find-WindowsProgram([string]$Tool,[string]$Location) {
    # A program the user installed and chose: a Windows executable (MZ header).
    $result=@{tool=$Tool;found=$false;compatible=$false;path='';executable='';version='';runtime='';managed=$false;
        reason='Choose the program you installed, then check again.'}
    if (-not $Location) { return $result }
    if (-not (Test-Path -LiteralPath $Location -PathType Leaf)) {
        $result.reason='That file does not exist. Choose the program you installed.'
        return $result
    }
    $stream=[IO.File]::OpenRead($Location)
    try { $pe=($stream.ReadByte() -eq 77 -and $stream.ReadByte() -eq 90) } finally {$stream.Dispose()}
    $result.found=$true
    $result.path=Split-Path -Parent $Location
    $result.executable=$Location
    $result.version=[string]([Diagnostics.FileVersionInfo]::GetVersionInfo($Location).ProductVersion)
    if ($pe -and [IO.Path]::GetExtension($Location) -eq '.exe') {
        $result.compatible=$true
        $result.reason='A Windows program. Whether the simulator talks to the autopilot is checked when the workspace starts.'
    } else {
        $result.reason='That file is not a Windows program. Choose the .exe you start the simulator with.'
    }
    return $result
}
function Find-WindowsTool([string]$Tool,[string]$Location,$Check) {
    if ($Check -and $Check.kind -eq 'windows-folder') { return Find-WindowsFolder $Tool $Location $Check }
    if ($Check -and $Check.kind -eq 'windows-file') { return Find-WindowsProgram $Tool $Location }
    $paths = @()
    $fallback=$null
    if ($Location) { $paths += $Location }
    if (-not $Location -and $Tool -eq 'qgc') {
        $paths += @((Join-Path $env:ProgramFiles 'QGroundControl\bin\QGroundControl.exe'),
                    (Join-Path $env:ProgramFiles 'QGroundControl\QGroundControl.exe'),
                    (Join-Path $env:LOCALAPPDATA 'QGroundControl\bin\QGroundControl.exe'))
    }
    foreach ($path in $paths) {
        if (Test-Path -LiteralPath $path -PathType Leaf) {
            $compatible=$false; $version=''; $runtime=''
            $reason='The selected file is not a verified tool. Choose the actual executable.'
            if ($Tool -eq 'qgc' -and [IO.Path]::GetExtension($path) -eq '.exe') {
                $info=[Diagnostics.FileVersionInfo]::GetVersionInfo($path)
                $version=[string]$info.ProductVersion
                $stream=[IO.File]::OpenRead($path)
                try { $pe=($stream.ReadByte() -eq 77 -and $stream.ReadByte() -eq 90) } finally {$stream.Dispose()}
                $compatible=$pe -and $info.ProductName -eq 'QGroundControl' -and $version -match '^\d+\.'
                if ($compatible) {$reason='QGroundControl executable and product version verified. Vehicle connection remains a separate check.'}
            }
            $result=@{tool=$Tool;found=$true;compatible=[bool]$compatible;path=$path;executable=$path;version=$version;runtime=$runtime;managed=$false;reason=$reason}
            if($compatible -or $Location){return $result};$fallback=$result
        }
    }
    if($fallback){return $fallback}
    return @{tool=$Tool;found=$false;compatible=$false;path='';executable='';version='';managed=$false;reason='No executable found. Select its full path manually.'}
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
