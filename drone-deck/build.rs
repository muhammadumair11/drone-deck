use std::{env, path::PathBuf, process::Command};

fn main() {
    println!("cargo:rerun-if-changed=assets/drone-deck.ico");
    println!("cargo:rerun-if-env-changed=RC");
    if env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    assert_eq!(
        env::var("CARGO_CFG_TARGET_ENV").as_deref(),
        Ok("msvc"),
        "Windows icon embedding requires the MSVC toolchain and Windows SDK"
    );
    let rc = env::var_os("RC").map(PathBuf::from).unwrap_or_else(|| {
        let kits = PathBuf::from(env::var_os("ProgramFiles(x86)").expect("ProgramFiles(x86)"))
            .join("Windows Kits/10/bin");
        let mut candidates: Vec<_> = std::fs::read_dir(kits)
            .expect("Windows SDK is required")
            .filter_map(Result::ok)
            .map(|entry| entry.path().join("x64/rc.exe"))
            .filter(|path| path.is_file())
            .collect();
        candidates.sort();
        candidates
            .pop()
            .expect("Windows SDK rc.exe was not found; set RC to its path")
    });
    let out = PathBuf::from(env::var_os("OUT_DIR").unwrap());
    let icon =
        PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").unwrap()).join("assets/drone-deck.ico");
    let resource = out.join("drone-deck.rc");
    let compiled = out.join("drone-deck.res");
    std::fs::write(
        &resource,
        format!(
            "1 ICON \"{}\"\n",
            icon.display().to_string().replace('\\', "/")
        ),
    )
    .expect("write icon resource");
    let status = Command::new(rc)
        .arg("/nologo")
        .arg("/fo")
        .arg(&compiled)
        .arg(&resource)
        .status()
        .expect("run Windows resource compiler");
    assert!(status.success(), "Windows icon resource compilation failed");
    println!("cargo:rustc-link-arg-bin=drone-deck={}", compiled.display());
}
