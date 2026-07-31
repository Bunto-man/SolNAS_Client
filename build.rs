fn main() {
    if std::env::var("CARGO_CFG_TARGET_OS").unwrap() == "windows" {
        let mut res = winres::WindowsResource::new();

        // Point this to exactly where your .ico file lives
        res.set_icon("Assets/Icon.ico");

        // Tell cargo to bundle it into the .exe
        res.compile().unwrap();
        if let Err(e) = res.compile() {
            println!("cargo:warning=Failed to compile icon resource: {}", e);
        }
    }
}
// Tell cargo to bundle it, but don't panic if it fails!
