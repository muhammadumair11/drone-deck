function Select-ToolLocation([string]$Tool,[string]$Distro,[string]$Pick) {
    # $Pick comes from the recipe: wsl-folder, wsl-file, windows-folder or windows-file.
    if ($Pick -notin @('wsl-folder','wsl-file','windows-folder','windows-file')) { throw 'This tool has no location to choose.' }
    Add-Type -AssemblyName System.Windows.Forms
    $linux = $Pick.StartsWith('wsl-')
    if ($Pick.EndsWith('-folder')) {
        $dialog=New-Object Windows.Forms.FolderBrowserDialog
        $dialog.Description=if($linux){'Select the existing tool folder inside Linux: browse to \\wsl.localhost\' + $Distro}else{'Select the folder where the tool is installed.'}
        $dialog.ShowNewFolderButton=$false
        try {$value=if($dialog.ShowDialog() -eq 'OK'){$dialog.SelectedPath}else{''}}finally{$dialog.Dispose()}
    }else{
        $dialog=New-Object Windows.Forms.OpenFileDialog
        $dialog.Title=if($linux){'Select the existing tool file inside Linux (\\wsl.localhost\' + $Distro + ')'}else{'Select the installed program'}
        $dialog.CheckFileExists=$true
        $dialog.Filter=if($linux){'Any file|*'}else{'Program|*.exe'}
        try {$value=if($dialog.ShowDialog() -eq 'OK'){$dialog.FileName}else{''}}finally{$dialog.Dispose()}
    }
    if($value -and $linux) {
        $prefix='\\wsl.localhost\' + $Distro + '\'
        $legacy='\\wsl$\' + $Distro + '\'
        if($value.StartsWith($prefix,[StringComparison]::OrdinalIgnoreCase)){$value='/' + $value.Substring($prefix.Length).Replace('\','/')}
        elseif($value.StartsWith($legacy,[StringComparison]::OrdinalIgnoreCase)){$value='/' + $value.Substring($legacy.Length).Replace('\','/')}
        elseif($value -match '^[A-Za-z]:'){ $value=To-LinuxPath $value }
        else{throw ('Select a location inside the ' + $Distro + ' distribution, under \\wsl.localhost\' + $Distro + '.')}
    }
    return @{tool=$Tool;location=$value}
}
