use eframe::egui;

pub const ICON_FILES: [&str; 8] = [
    "layout-dashboard.svg",
    "info.svg",
    "refresh-cw.svg",
    "languages.svg",
    "sun.svg",
    "moon.svg",
    "circle-check.svg",
    "triangle-alert.svg",
];

pub fn layout_dashboard() -> egui::ImageSource<'static> {
    egui::include_image!("../assets/icons/layout-dashboard.svg")
}

pub fn info() -> egui::ImageSource<'static> {
    egui::include_image!("../assets/icons/info.svg")
}

pub fn refresh_cw() -> egui::ImageSource<'static> {
    egui::include_image!("../assets/icons/refresh-cw.svg")
}

pub fn languages() -> egui::ImageSource<'static> {
    egui::include_image!("../assets/icons/languages.svg")
}

pub fn sun() -> egui::ImageSource<'static> {
    egui::include_image!("../assets/icons/sun.svg")
}

pub fn moon() -> egui::ImageSource<'static> {
    egui::include_image!("../assets/icons/moon.svg")
}

pub fn circle_check() -> egui::ImageSource<'static> {
    egui::include_image!("../assets/icons/circle-check.svg")
}

pub fn triangle_alert() -> egui::ImageSource<'static> {
    egui::include_image!("../assets/icons/triangle-alert.svg")
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::Path;

    use super::*;

    #[test]
    fn declared_lucide_assets_exist_and_are_safe_standalone_svgs() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("assets/icons");
        assert_eq!(ICON_FILES.len(), 8);

        for file in ICON_FILES {
            let path = root.join(file);
            let contents = fs::read_to_string(&path)
                .unwrap_or_else(|error| panic!("read {}: {error}", path.display()));
            let normalized = contents.to_ascii_lowercase();

            assert!(contents.contains("viewBox=\"0 0 24 24\""), "{file}");
            assert!(contents.contains("fill=\"none\""), "{file}");
            assert!(contents.contains("stroke=\"currentColor\""), "{file}");
            for forbidden in [
                "<script",
                "<foreignobject",
                "href=\"http",
                "href='http",
                "url(http",
                "gradient",
            ] {
                assert!(!normalized.contains(forbidden), "{file}: {forbidden}");
            }
        }
    }

    #[test]
    fn lucide_license_is_packaged_with_icons() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("assets/icons/LICENSE");
        let license = fs::read_to_string(path).unwrap();
        assert!(license.starts_with("ISC License"));
        assert!(license.contains("Lucide Contributors"));
    }
}
