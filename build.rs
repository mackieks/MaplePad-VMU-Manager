fn main() {
    println!("cargo:rerun-if-changed=assets/maple.ico");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        winresource::WindowsResource::new()
            .set_icon("assets/maple.ico")
            .set("ProductName", "MaplePad VMU Manager")
            .set("FileDescription", "MaplePad VMU Manager")
            .compile()
            .expect("Windows icon resource");
    }
}
