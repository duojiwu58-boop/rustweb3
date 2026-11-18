use std::{env, fs, path::PathBuf, process::Command};

fn main() {
    println!("cargo:rerun-if-env-changed=RC");
    let assets = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").unwrap())
        .join("../../../desktop/assets");
    for name in ["wallet.rc", "wallet.ico"] {
        println!("cargo:rerun-if-changed={}", assets.join(name).display());
    }
    if env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    let compiler = env::var_os("RC").map(PathBuf::from).unwrap_or_else(|| {
        let kits = PathBuf::from(env::var_os("ProgramFiles(x86)").expect("Windows SDK or RC required"))
            .join("Windows Kits/10/bin");
        let mut versions: Vec<_> = fs::read_dir(kits).expect("Windows SDK resource compiler required")
            .filter_map(Result::ok).map(|e| e.path().join("x64/rc.exe"))
            .filter(|p| p.is_file()).collect();
        versions.sort();
        versions.pop().expect("Set RC to the Windows SDK rc.exe")
    });
    let resource = PathBuf::from(env::var_os("OUT_DIR").unwrap()).join("wallet.res");
    let status = Command::new(compiler).current_dir(&assets)
        .arg("/nologo").arg("/fo").arg(&resource).arg("wallet.rc")
        .status().expect("run Windows resource compiler");
    assert!(status.success(), "wallet icon resource compilation failed");
    println!("cargo:rustc-link-arg-bin=wallet-launcher={}", resource.display());
}
