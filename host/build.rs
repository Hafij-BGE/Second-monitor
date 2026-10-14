fn main() {
    println!("cargo:rerun-if-changed=../assets/extendo.ico");
    // Embed the icon and version info in the Windows .exe.
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        let mut res = winresource::WindowsResource::new();
        res.set_icon("../assets/extendo.ico")
            .set("ProductName", "Extendo")
            .set("FileDescription", "Extendo - tablet as a second monitor")
            .set("CompanyName", "Md Hafijur Rahman")
            .set("LegalCopyright", "Md Hafijur Rahman");
        if let Err(e) = res.compile() {
            println!("cargo:warning=could not embed Windows icon: {e}");
        }
    }
}
