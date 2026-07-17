#[test]
fn native_manager_uses_windows_gui_subsystem() {
    let main_rs = include_str!("../src/main.rs");
    assert!(main_rs.contains("#![cfg_attr(windows, windows_subsystem = \"windows\")]"));
}
