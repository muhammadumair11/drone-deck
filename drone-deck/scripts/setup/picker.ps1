function Select-ToolLocation([string]$Tool,[string]$Distro) {
    Add-Type -AssemblyName System.Windows.Forms
    if($Tool -in @('px4','mavsdk','ardupilot','ros2')) {
        $dialog=New-Object Windows.Forms.FolderBrowserDialog
        $dialog.Description='Select the existing tool folder. For Linux, browse to \\wsl.localhost\' + $Distro
        $dialog.ShowNewFolderButton=$false
        try {$value=if($dialog.ShowDialog() -eq 'OK'){$dialog.SelectedPath}else{''}}finally{$dialog.Dispose()}
    }else{
        $dialog=New-Object Windows.Forms.OpenFileDialog
        $dialog.Title='Select existing tool executable or visualizer JAR'
        $dialog.CheckFileExists=$true
        $dialog.Filter=if($Tool -eq 'qgc'){'QGroundControl executable|*.exe'}elseif($Tool -eq 'jmavsim'){'jMAVSim visualizer|*.jar'}else{'Executable|*'}
        try {$value=if($dialog.ShowDialog() -eq 'OK'){$dialog.FileName}else{''}}finally{$dialog.Dispose()}
    }
    if($value -and $Tool -notin @('qgc','jmavsim')) {
        $prefix='\\wsl.localhost\' + $Distro + '\'
        $legacy='\\wsl$\' + $Distro + '\'
        if($value.StartsWith($prefix,[StringComparison]::OrdinalIgnoreCase)){$value='/' + $value.Substring($prefix.Length).Replace('\','/')}
        elseif($value.StartsWith($legacy,[StringComparison]::OrdinalIgnoreCase)){$value='/' + $value.Substring($legacy.Length).Replace('\','/')}
        elseif($value -match '^[A-Za-z]:'){ $value=To-LinuxPath $value }
        else{throw 'Select a location inside the chosen WSL distribution, or enter its Linux path manually.'}
    }
    return @{tool=$Tool;location=$value}
}
