$ErrorActionPreference = 'Stop'
[Console]::OutputEncoding = [Text.UTF8Encoding]::new($false)
if (-not ('DroneSetupNative' -as [type])) {
Add-Type @'
using System;
using System.Diagnostics;
using System.IO;
using System.Text;
public sealed class DroneSetupResult { public int code; public string output; public string error; }
public static class DroneSetupNative {
    public static DroneSetupResult Run(string program,string arguments,int timeout,string log,bool preserve) {
        var info=new ProcessStartInfo(program,arguments) { UseShellExecute=false,CreateNoWindow=true,
            RedirectStandardInput=true,RedirectStandardOutput=true,RedirectStandardError=true,
            StandardOutputEncoding=Encoding.UTF8,StandardErrorEncoding=Encoding.UTF8 };
        var output=new StringBuilder(); var error=new StringBuilder(); var gate=new object();
        var lastOutput=DateTime.UtcNow; var started=DateTime.UtcNow;
        var waiting=String.IsNullOrEmpty(log)?null:log+".waiting";
        if(waiting!=null && File.Exists(waiting))File.Delete(waiting);
        using(var process=new Process { StartInfo=info }) {
            DataReceivedEventHandler read=(sender,args)=> { if(args.Data==null)return;
                var text=args.Data.Replace("\0",""); lock(gate) {
                    lastOutput=DateTime.UtcNow;
                    var target=Object.ReferenceEquals(sender,process)?output:error;
                    if(target.Length<2000000)target.AppendLine(text);
                    if(!String.IsNullOrEmpty(log))File.AppendAllText(log,text+Environment.NewLine,Encoding.UTF8);
                }};
            process.OutputDataReceived+=(sender,args)=>read(process,args);
            process.ErrorDataReceived+=(sender,args)=>read(null,args);
            process.Start();process.StandardInput.Close();process.BeginOutputReadLine();process.BeginErrorReadLine();
            while(!process.WaitForExit(250)) {
                bool expired=(DateTime.UtcNow-started).TotalSeconds>=timeout;
                if(expired && !preserve) {
                    process.Kill();process.WaitForExit();
                    throw new TimeoutException("Command exceeded its time limit. Review the log before retrying.");
                }
                if(preserve && waiting!=null) {
                    lock(gate) {
                        if(expired || (DateTime.UtcNow-lastOutput).TotalSeconds>300)
                            File.WriteAllText(waiting,"The current installation step is taking longer than expected or has produced no output for five minutes. Its process is preserved to protect package state. Check its log or visible installer; cancellation takes effect after this step.");
                        else if(File.Exists(waiting))File.Delete(waiting);
                    }
                }
            }
            process.WaitForExit();
            if(waiting!=null && File.Exists(waiting))File.Delete(waiting);
            return new DroneSetupResult { code=process.ExitCode,output=output.ToString(),error=error.ToString() };
        }
    }
}
'@
}

function Write-JsonFile($Path, $Value) {
    $temporary = $Path + '.' + [guid]::NewGuid().ToString('N') + '.tmp'
    [IO.File]::WriteAllText($temporary, ($Value | ConvertTo-Json -Depth 20), [Text.UTF8Encoding]::new($false))
    Move-Item -LiteralPath $temporary -Destination $Path -Force
}
function Quote-Native([string]$Value) {
    if ($Value -ne '' -and $Value -notmatch '[\s"]') { return $Value }
    # Windows CommandLineToArgvW escaping, preserving embedded quotes and trailing slashes.
    '"' + [regex]::Replace([regex]::Replace($Value, '(\\*)"', '$1$1\"'), '(\\+)$', '$1$1') + '"'
}
function Invoke-Native([string]$Program, [string[]]$Arguments, [int]$Timeout = 30, [string]$Log = '', [switch]$PreserveOnTimeout) {
    $argumentsLine = ($Arguments | ForEach-Object { Quote-Native $_ }) -join ' '
    $result = [DroneSetupNative]::Run($Program,$argumentsLine,$Timeout,$Log,[bool]$PreserveOnTimeout)
    return @{code=$result.code;out=$result.output;err=$result.error}
}
function Get-Wsl { Join-Path $env:WINDIR 'System32\wsl.exe' }
function To-LinuxPath([string]$Path) {
    if ($Path -notmatch '^([A-Za-z]):[\\/](.*)$') { throw 'Runtime scripts require a Windows drive path.' }
    '/mnt/' + $Matches[1].ToLower() + '/' + $Matches[2].Replace('\','/')
}
function Get-Recipes {
    $items = Get-Content -Raw -LiteralPath (Join-Path $PSScriptRoot 'catalog.json') | ConvertFrom-Json
    foreach ($item in $items) { Write-Output $item }
}
function Get-InstalledDistros {
    $result = Invoke-Native (Get-Wsl) @('--list','--verbose') 15
    if ($result.code -ne 0) { return @() }
    $rows = @()
    foreach ($line in ($result.out -split "`n")) {
        if ($line -match '^\s*(?:\*\s*)?(\S+)\s+(.+?)\s+([12])\s*$') {
            $rows += @{name=$Matches[1];state=$Matches[2].Trim();version=[int]$Matches[3]}
        }
    }
    return $rows
}
function Get-OnlineDistros {
    $result = Invoke-Native (Get-Wsl) @('--list','--online') 35
    if ($result.code -ne 0) { throw ('Online distribution catalog unavailable: ' + $result.err + $result.out) }
    $rows = @()
    foreach ($line in ($result.out -split "`n")) {
        if ($line -match '^\s*([A-Za-z][A-Za-z0-9._-]+)\s{2,}(.+?)\s*$' -and $Matches[1] -ne 'NAME') {
            $rows += @{name=$Matches[1];label=$Matches[2];support=$(if ($Matches[1] -eq 'Ubuntu-24.04') {'Recommended'} else {'Advanced / recipe compatibility not verified'})}
        }
    }
    if (-not $rows) { throw 'No distribution identifiers could be read from the Microsoft catalog.' }
    return $rows
}
function Test-JobAlive($State) {
    if (-not $State.pid -or -not $State.started_process) { return $false }
    try {
        $process = Get-Process -Id ([int]$State.pid) -ErrorAction Stop
        return (-not $process.HasExited) -and ($process.StartTime.ToUniversalTime().ToString('o') -eq $State.started_process)
    } catch { return $false }
}
function Get-Jobs([string]$Root) {
    $folder = Join-Path $Root 'setup\jobs'
    if (-not (Test-Path -LiteralPath $folder)) { return @() }
    $rows = @()
    foreach ($directory in @(Get-ChildItem -LiteralPath $folder -Directory | Sort-Object Name -Descending | Select-Object -First 50)) {
        try {
            $state = Get-Content -Raw -LiteralPath (Join-Path $directory.FullName 'state.json') | ConvertFrom-Json
            $alive = Test-JobAlive $state
            if ($state.status -in @('Queued','Running','Downloading','Installing','Checking') -and -not $alive) {
                if (([DateTime]::UtcNow - [DateTime]::Parse($state.updated).ToUniversalTime()).TotalSeconds -gt 20) {
                    $state.status = 'Interrupted'
                    $state.message = 'Worker is no longer running. Review its log, then retry; completed work will be checked first.'
                }
            }
            $log = Join-Path $directory.FullName 'output.log'
            $state | Add-Member -NotePropertyName alive -NotePropertyValue $alive -Force
            $state | Add-Member -NotePropertyName log_tail -NotePropertyValue $(if (Test-Path -LiteralPath $log) {(Get-Content -LiteralPath $log -Tail 35) -join "`n"} else {''}) -Force
            $rows += $state
        } catch { }
    }
    return $rows
}
