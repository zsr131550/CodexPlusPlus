use std::fmt;
use std::fs;
use std::path::PathBuf;
use std::sync::Arc;

use eframe::egui;

const CJK_FONT_NAME: &str = "codex_plus_cjk";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FontLoadError {
    attempted: Vec<PathBuf>,
}

impl FontLoadError {
    pub fn attempted(&self) -> &[PathBuf] {
        &self.attempted
    }
}

impl fmt::Display for FontLoadError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "no readable nonempty CJK font in {} candidates",
            self.attempted.len()
        )
    }
}

impl std::error::Error for FontLoadError {}

pub fn cjk_font_candidates() -> Vec<PathBuf> {
    platform_cjk_font_candidates()
}

#[cfg(target_os = "windows")]
fn platform_cjk_font_candidates() -> Vec<PathBuf> {
    let windows = std::env::var_os("WINDIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(r"C:\Windows"));
    let fonts = windows.join("Fonts");
    ["simhei.ttf", "msyh.ttc", "msyhbd.ttc", "simsun.ttc"]
        .into_iter()
        .map(|name| fonts.join(name))
        .collect()
}

#[cfg(target_os = "macos")]
fn platform_cjk_font_candidates() -> Vec<PathBuf> {
    [
        "/System/Library/Fonts/PingFang.ttc",
        "/System/Library/Fonts/STHeiti Medium.ttc",
        "/System/Library/Fonts/Hiragino Sans GB.ttc",
    ]
    .into_iter()
    .map(PathBuf::from)
    .collect()
}

#[cfg(not(any(target_os = "windows", target_os = "macos")))]
fn platform_cjk_font_candidates() -> Vec<PathBuf> {
    [
        "/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc",
        "/usr/share/fonts/opentype/noto/NotoSansCJK-VF.ttc",
        "/usr/share/fonts/truetype/wqy/wqy-microhei.ttc",
    ]
    .into_iter()
    .map(PathBuf::from)
    .collect()
}

pub fn load_cjk_font() -> Result<Vec<u8>, FontLoadError> {
    load_first_nonempty(&cjk_font_candidates())
}

fn load_first_nonempty(candidates: &[PathBuf]) -> Result<Vec<u8>, FontLoadError> {
    for path in candidates {
        if let Ok(bytes) = fs::read(path)
            && !bytes.is_empty()
        {
            return Ok(bytes);
        }
    }
    Err(FontLoadError {
        attempted: candidates.to_vec(),
    })
}

pub fn install_cjk_font(ctx: &egui::Context, bytes: Vec<u8>) {
    let mut fonts = egui::FontDefinitions::default();
    fonts.font_data.insert(
        CJK_FONT_NAME.to_owned(),
        Arc::new(egui::FontData::from_owned(bytes)),
    );
    fonts
        .families
        .entry(egui::FontFamily::Proportional)
        .or_default()
        .insert(0, CJK_FONT_NAME.to_owned());
    fonts
        .families
        .entry(egui::FontFamily::Monospace)
        .or_default()
        .push(CJK_FONT_NAME.to_owned());
    ctx.set_fonts(fonts);
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;

    #[test]
    fn platform_cjk_font_candidates_are_ordered_and_nonempty() {
        let candidates = cjk_font_candidates();
        assert!(!candidates.is_empty());

        #[cfg(target_os = "windows")]
        assert_eq!(
            candidates
                .iter()
                .take(4)
                .map(|path| path.file_name().unwrap().to_string_lossy().to_string())
                .collect::<Vec<_>>(),
            ["simhei.ttf", "msyh.ttc", "msyhbd.ttc", "simsun.ttc"]
        );

        #[cfg(target_os = "macos")]
        assert_eq!(
            candidates[0],
            std::path::PathBuf::from("/System/Library/Fonts/PingFang.ttc")
        );
    }

    #[test]
    fn font_loader_skips_empty_candidate_and_uses_first_nonempty_file() {
        let temp = tempfile::tempdir().unwrap();
        let empty = temp.path().join("empty.ttc");
        let usable = temp.path().join("usable.ttc");
        fs::write(&empty, []).unwrap();
        fs::write(&usable, [1, 2, 3, 4]).unwrap();

        assert_eq!(
            load_first_nonempty(&[empty, usable]).unwrap(),
            vec![1, 2, 3, 4]
        );
    }
}
