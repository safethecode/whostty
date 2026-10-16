#[path = "src/icon.rs"]
mod icon;

fn main() {
    println!("cargo:rerun-if-changed=src/icon.rs");
    println!("cargo:rerun-if-changed=whostty.manifest");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    let output = std::path::PathBuf::from(std::env::var_os("OUT_DIR").unwrap()).join("whostty.ico");
    let mut ico = vec![0, 0, 1, 0, 1, 0, 32, 32, 0, 0, 1, 0, 32, 0];
    ico.extend_from_slice(&(40_u32 + 4096 + 128).to_le_bytes());
    ico.extend_from_slice(&22_u32.to_le_bytes());
    for value in [40_u32, 32, 64] {
        ico.extend_from_slice(&value.to_le_bytes());
    }
    ico.extend_from_slice(&[1, 0, 32, 0]);
    for value in [0_u32, 4096, 0, 0, 0, 0] {
        ico.extend_from_slice(&value.to_le_bytes());
    }
    let pixels = icon::rgba();
    for row in (0..32).rev() {
        for pixel in pixels[row * 128..(row + 1) * 128].chunks_exact(4) {
            ico.extend_from_slice(&[pixel[2], pixel[1], pixel[0], pixel[3]]);
        }
    }
    ico.extend_from_slice(&[0; 128]);
    std::fs::write(&output, ico).expect("Write Windows icon");
    winresource::WindowsResource::new()
        .set("ProductName", "Whostty")
        .set("FileDescription", "Whostty terminal")
        .set("OriginalFilename", "whostty.exe")
        .set_icon(output.to_str().unwrap())
        .set_manifest_file("whostty.manifest")
        .compile()
        .expect("Compile Windows resources (install the Windows SDK)");
}
