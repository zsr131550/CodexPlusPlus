use eframe::egui;

pub const ICON_FILES: [&str; 29] = [
    "layout-dashboard.svg",
    "info.svg",
    "refresh-cw.svg",
    "languages.svg",
    "sun.svg",
    "moon.svg",
    "circle-check.svg",
    "triangle-alert.svg",
    "server-cog.svg",
    "plus.svg",
    "copy.svg",
    "chevron-up.svg",
    "chevron-down.svg",
    "trash-2.svg",
    "panel-left-close.svg",
    "panel-left-open.svg",
    "save.svg",
    "eye.svg",
    "eye-off.svg",
    "stethoscope.svg",
    "pencil.svg",
    "wrench.svg",
    "message-circle.svg",
    "file-code-2.svg",
    "folder-git-2.svg",
    "folder-open.svg",
    "file-search.svg",
    "play.svg",
    "rotate-ccw.svg",
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

pub fn server_cog() -> egui::ImageSource<'static> {
    egui::include_image!("../assets/icons/server-cog.svg")
}

pub fn plus() -> egui::ImageSource<'static> {
    egui::include_image!("../assets/icons/plus.svg")
}

pub fn copy() -> egui::ImageSource<'static> {
    egui::include_image!("../assets/icons/copy.svg")
}

pub fn chevron_up() -> egui::ImageSource<'static> {
    egui::include_image!("../assets/icons/chevron-up.svg")
}

pub fn chevron_down() -> egui::ImageSource<'static> {
    egui::include_image!("../assets/icons/chevron-down.svg")
}

pub fn trash_2() -> egui::ImageSource<'static> {
    egui::include_image!("../assets/icons/trash-2.svg")
}

pub fn panel_left_close() -> egui::ImageSource<'static> {
    egui::include_image!("../assets/icons/panel-left-close.svg")
}

pub fn panel_left_open() -> egui::ImageSource<'static> {
    egui::include_image!("../assets/icons/panel-left-open.svg")
}

pub fn save() -> egui::ImageSource<'static> {
    egui::include_image!("../assets/icons/save.svg")
}

pub fn eye() -> egui::ImageSource<'static> {
    egui::include_image!("../assets/icons/eye.svg")
}

pub fn eye_off() -> egui::ImageSource<'static> {
    egui::include_image!("../assets/icons/eye-off.svg")
}

pub fn stethoscope() -> egui::ImageSource<'static> {
    egui::include_image!("../assets/icons/stethoscope.svg")
}

pub fn pencil() -> egui::ImageSource<'static> {
    egui::include_image!("../assets/icons/pencil.svg")
}

pub fn wrench() -> egui::ImageSource<'static> {
    egui::include_image!("../assets/icons/wrench.svg")
}

pub fn message_circle() -> egui::ImageSource<'static> {
    egui::include_image!("../assets/icons/message-circle.svg")
}

pub fn file_code_2() -> egui::ImageSource<'static> {
    egui::include_image!("../assets/icons/file-code-2.svg")
}

pub fn folder_git_2() -> egui::ImageSource<'static> {
    egui::include_image!("../assets/icons/folder-git-2.svg")
}

pub fn folder_open() -> egui::ImageSource<'static> {
    egui::include_image!("../assets/icons/folder-open.svg")
}

pub fn file_search() -> egui::ImageSource<'static> {
    egui::include_image!("../assets/icons/file-search.svg")
}

pub fn play() -> egui::ImageSource<'static> {
    egui::include_image!("../assets/icons/play.svg")
}

pub fn rotate_ccw() -> egui::ImageSource<'static> {
    egui::include_image!("../assets/icons/rotate-ccw.svg")
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::Path;

    use super::*;

    #[test]
    fn declared_lucide_assets_exist_and_are_safe_standalone_svgs() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("assets/icons");
        assert_eq!(ICON_FILES.len(), 29);

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
