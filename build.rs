use std::{env, fs, path::Path};

fn main() {
    let target_os = env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    if target_os == "macos" {
        println!("cargo:rerun-if-changed=src/platform/macos/warp_ime_shim.m");
        println!("cargo:rustc-link-lib=framework=AppKit");

        cc::Build::new()
            .file("src/platform/macos/warp_ime_shim.m")
            .compile("nexshell_warp_ime_shim");
    }

    if target_os == "windows" {
        embed_windows_resource();
    }
}

fn embed_windows_resource() {
    const ICON_SOURCES: &[(&str, u8, u8)] = &[
        ("assets/AppIcon.windows/icon_16x16.png", 16, 16),
        ("assets/AppIcon.windows/icon_24x24.png", 24, 24),
        ("assets/AppIcon.windows/icon_32x32.png", 32, 32),
        ("assets/AppIcon.windows/icon_48x48.png", 48, 48),
        ("assets/AppIcon.windows/icon_64x64.png", 64, 64),
        ("assets/AppIcon.windows/icon_128x128.png", 128, 128),
        ("assets/AppIcon.windows/icon_256x256.png", 0, 0),
    ];

    for (path, _, _) in ICON_SOURCES {
        println!("cargo:rerun-if-changed={path}");
    }

    let out_dir = env::var("OUT_DIR").expect("OUT_DIR is set by Cargo");
    let out_dir = Path::new(&out_dir);
    let icon_path = out_dir.join("AppIcon.ico");
    write_ico_with_png_frames(&icon_path, ICON_SOURCES);

    let rc_path = out_dir.join("nexshell.rc");
    fs::write(
        &rc_path,
        r#"#pragma code_page(65001)
#define IDI_ICON 101

IDI_ICON ICON "AppIcon.ico"
"#,
    )
    .expect("write Windows resource file");

    embed_resource::compile(&rc_path, embed_resource::NONE)
        .manifest_optional()
        .expect("embed Windows app icon resource");
}

fn write_ico_with_png_frames(icon_path: &Path, sources: &[(&str, u8, u8)]) {
    let frames = sources
        .iter()
        .map(|(path, width, height)| {
            let bytes = fs::read(path).unwrap_or_else(|err| panic!("read {path}: {err}"));
            (*width, *height, bytes)
        })
        .collect::<Vec<_>>();

    let count = frames.len();
    let mut ico = Vec::new();
    ico.extend_from_slice(&0u16.to_le_bytes());
    ico.extend_from_slice(&1u16.to_le_bytes());
    ico.extend_from_slice(&(count as u16).to_le_bytes());

    let mut image_offset = 6 + count * 16;
    for (width, height, bytes) in &frames {
        ico.push(*width);
        ico.push(*height);
        ico.push(0);
        ico.push(0);
        ico.extend_from_slice(&1u16.to_le_bytes());
        ico.extend_from_slice(&32u16.to_le_bytes());
        ico.extend_from_slice(&(bytes.len() as u32).to_le_bytes());
        ico.extend_from_slice(&(image_offset as u32).to_le_bytes());
        image_offset += bytes.len();
    }

    for (_, _, bytes) in frames {
        ico.extend_from_slice(&bytes);
    }

    fs::write(icon_path, ico).expect("write Windows icon file");
}
