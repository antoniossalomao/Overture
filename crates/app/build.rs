use std::path::Path;

fn main() {
    #[cfg(target_os = "windows")]
    {
        let ico_path = Path::new("../../assets/overture.ico");
        println!("cargo:rerun-if-changed=../../assets/overture.ico");
        if ico_path.exists() {
            let mut res = winresource::WindowsResource::new();
            res.set_icon("../../assets/overture.ico");
            res.set("ProductName", "Overture");
            res.set("FileDescription", "Overture — Projeção para Cultos");
            res.set("CompanyName", "Overture Project");
            res.set("LegalCopyright", "Copyright (C) 2026 Overture Contributors (GPL-3.0)");
            let _ = res.compile();
        }
    }
}
