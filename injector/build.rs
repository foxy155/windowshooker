use std::process::Command;

fn main() {
    #[cfg(windows)]
    {
        println!("cargo:rerun-if-changed=build.rs");

        // Ask PowerShell which processes currently have hook_dll.dll loaded.
        // Any match means the file is locked and the build will fail.
        let ps_script = r#"
$ErrorActionPreference = 'SilentlyContinue'
$procs = Get-Process | Where-Object {
    try { $_.Modules.FileName -like '*hook_dll*' } catch { $false }
}
if ($procs) {
    foreach ($p in $procs) {
        Write-Host "killing locked process: $($p.Id) $($p.ProcessName)"
        Stop-Process -Id $p.Id -Force
    }
}
"#;

        let _ = Command::new("powershell")
            .args(["-NoProfile", "-Command", ps_script])
            .status();

        // Give Windows a moment to release the file handle.
        std::thread::sleep(std::time::Duration::from_millis(200));
    }
}