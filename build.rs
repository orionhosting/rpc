fn main() {
    println!("cargo:rerun-if-changed=./icon.png");

    // Tray icon

    let img = image::open("./icon.png").unwrap().to_rgba8();
    img.save_with_format("./icon.ico", image::ImageFormat::Ico)
        .unwrap();

    let (w, h) = img.dimensions();
    let mut raw = img.into_raw();

    let mut out = Vec::with_capacity(raw.len() + 8);
    out.extend_from_slice(&w.to_le_bytes());
    out.extend_from_slice(&h.to_le_bytes());
    out.append(&mut raw);

    std::fs::write("./icon.bin", out).unwrap();

    // Binary icon

    if std::env::var("CARGO_CFG_TARGET_OS").unwrap() == "windows" {
        let mut res = winresource::WindowsResource::new();
        res.set_icon("./icon.ico");
        // .set("InternalName", "TEST.EXE")
        // manually set version 1.0.0.0
        // .set_version_info(winresource::VersionInfo::PRODUCTVERSION, 0x0001000000000000);
        res.compile().unwrap();
    }
}
