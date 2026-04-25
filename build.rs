#[cfg(windows)]
fn main() {
    use std::{env, fs, path::PathBuf};

    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").expect("manifest dir"));
    let out_dir = PathBuf::from(env::var("OUT_DIR").expect("out dir"));
    let icon_path = manifest_dir.join("assets").join("pane-icon.ico");
    let version = env::var("CARGO_PKG_VERSION").unwrap_or_else(|_| "0.1.0".to_string());
    let version_parts = parse_version(&version);

    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed={}", icon_path.display());

    let icon = icon_path.display().to_string().replace('\\', "\\\\");
    let rc_contents = format!(
        r#"
1 ICON "{icon}"

VS_VERSION_INFO VERSIONINFO
 FILEVERSION {major},{minor},{patch},{build}
 PRODUCTVERSION {major},{minor},{patch},{build}
 FILEFLAGSMASK 0x3fL
#ifdef _DEBUG
 FILEFLAGS 0x1L
#else
 FILEFLAGS 0x0L
#endif
 FILEOS 0x40004L
 FILETYPE 0x1L
 FILESUBTYPE 0x0L
BEGIN
  BLOCK "StringFileInfo"
  BEGIN
    BLOCK "040904B0"
    BEGIN
      VALUE "CompanyName", "Pane Project\0"
      VALUE "FileDescription", "Pane Windows launcher and managed Linux environment app\0"
      VALUE "FileVersion", "{version}.0\0"
      VALUE "InternalName", "pane.exe\0"
      VALUE "OriginalFilename", "pane.exe\0"
      VALUE "ProductName", "Pane\0"
      VALUE "ProductVersion", "{version}.0\0"
    END
  END
  BLOCK "VarFileInfo"
  BEGIN
    VALUE "Translation", 0x0409, 1200
  END
END
"#,
        icon = icon,
        version = version,
        major = version_parts[0],
        minor = version_parts[1],
        patch = version_parts[2],
        build = version_parts[3],
    );

    let rc_path = out_dir.join("pane-version.rc");
    fs::write(&rc_path, rc_contents).expect("write rc");
    embed_resource::compile(rc_path, embed_resource::NONE)
        .manifest_required()
        .expect("compile Pane Windows resources");
}

#[cfg(not(windows))]
fn main() {}

#[cfg(windows)]
fn parse_version(version: &str) -> [u16; 4] {
    let mut parts = [0u16; 4];
    for (index, segment) in version.split('.').take(4).enumerate() {
        parts[index] = segment.parse::<u16>().unwrap_or(0);
    }
    parts
}
