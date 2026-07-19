use codex_plus_manager_native::external_url::{ExternalUrl, ExternalUrlError};
use eframe::egui;

#[test]
fn accepts_only_absolute_http_and_https_urls_with_hosts() {
    for value in [
        "https://github.com/BigPizzaV3/CodexPlusPlus",
        "http://127.0.0.1:8080/scripts?q=one#two",
    ] {
        assert_eq!(ExternalUrl::parse(value).unwrap().as_str(), value);
    }

    for value in [
        "",
        "/relative",
        "//example.test/path",
        "mailto:test@example.test",
        "javascript:alert(1)",
        "file:///C:/private/file.txt",
        "data:text/plain,hello",
        "https:///missing-host",
        "https://example.test/path\nnext",
        "https://example.test/\u{0000}hidden",
    ] {
        assert_eq!(
            ExternalUrl::parse(value),
            Err(ExternalUrlError::Invalid),
            "{value:?}"
        );
    }
}

#[test]
fn debug_output_never_discloses_the_validated_url() {
    let url = ExternalUrl::parse(
        "https://example.test/private-path?token=private-query-sentinel#private-fragment",
    )
    .unwrap();

    let debug = format!("{url:?}");
    assert_eq!(debug, "ExternalUrl([validated])");
    assert!(!debug.contains("private-query-sentinel"));
    assert!(!debug.contains("private-path"));
}

#[test]
fn emitting_a_validated_url_produces_only_an_egui_open_url_command() {
    let url = ExternalUrl::parse("https://example.test/project").unwrap();
    let context = egui::Context::default();

    let output = context.run_ui(egui::RawInput::default(), |ui| {
        url.emit(ui.ctx());
    });

    assert_eq!(
        output.platform_output.commands,
        [egui::OutputCommand::OpenUrl(egui::OpenUrl::new_tab(
            "https://example.test/project"
        ))]
    );
}
