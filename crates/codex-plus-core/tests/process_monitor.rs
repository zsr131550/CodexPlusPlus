use codex_plus_core::process_monitor::{
    cdp_listening, codex_process_ids, filter_killable_launcher_processes,
    should_recover_stale_launcher,
};

#[cfg(windows)]
use codex_plus_core::process_monitor::{WindowsProcessInfo, find_codex_processes_from_snapshot};

#[test]
fn cdp_listening_returns_true_for_bound_loopback_port() {
    let listener = std::net::TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let port = listener.local_addr().unwrap().port();

    assert!(cdp_listening(port));
}

#[test]
fn cdp_listening_returns_true_for_bound_ipv6_loopback_port() {
    let listener = std::net::TcpListener::bind("[::1]:0").unwrap();
    let port = listener.local_addr().unwrap().port();

    assert!(cdp_listening(port));
}

#[test]
fn cdp_listening_returns_false_for_closed_port() {
    let port = {
        let listener = std::net::TcpListener::bind(("127.0.0.1", 0)).unwrap();
        listener.local_addr().unwrap().port()
    };

    assert!(!cdp_listening(port));
}

#[test]
fn codex_process_filter_keeps_only_windowsapps_codex_processes() {
    let processes = [
        (
            11,
            r"C:\Program Files\WindowsApps\OpenAI.Codex_1.0.0.0_x64__abc\app\Codex.exe",
        ),
        (12, r"C:\Tools\Codex.exe"),
        (
            13,
            r"C:\Program Files\WindowsApps\Other.App_1.0.0.0_x64__abc\app\Codex.exe",
        ),
    ];

    assert_eq!(codex_process_ids(processes), vec![11]);
}

#[test]
fn codex_process_filter_keeps_chatgpt_desktop_package_processes() {
    let processes = [
        (
            21,
            r"C:\Program Files\WindowsApps\OpenAI.ChatGPT-Desktop_1.2026.133.0_x64__abc\app\ChatGPT.exe",
        ),
        (
            22,
            r"C:\Program Files\WindowsApps\OpenAI.Codex_26.707.3748.0_x64__abc\app\ChatGPT.exe",
        ),
        (
            23,
            r"C:\Program Files\WindowsApps\OpenAI.ChatGPT-Desktop_1.2026.133.0_x64__abc\app\resources\ChatGPT.exe",
        ),
        (
            24,
            r"C:\Program Files\WindowsApps\Other.ChatGPT_1.0.0.0_x64__abc\app\ChatGPT.exe",
        ),
    ];

    assert_eq!(codex_process_ids(processes), vec![21, 22]);
}

#[test]
fn launcher_process_filter_protects_current_process_ancestry() {
    let processes = [
        (10, 0, "codex-plus-plus.exe"),
        (20, 10, "codex-plus-plus.exe"),
        (30, 20, "codex-plus-plus.exe"),
        (40, 10, "codex-plus-plus.exe"),
        (50, 10, "codex-plus-plus-manager.exe"),
    ];

    assert_eq!(filter_killable_launcher_processes(processes, 30), vec![40]);
}

#[test]
fn stale_launcher_recovery_only_runs_when_codex_and_cdp_are_absent() {
    assert!(should_recover_stale_launcher(false, false));
    assert!(!should_recover_stale_launcher(true, false));
    assert!(!should_recover_stale_launcher(false, true));
    assert!(!should_recover_stale_launcher(true, true));
}

#[cfg(windows)]
#[test]
fn find_codex_processes_finds_local_install_with_capital_c() {
    let processes = [WindowsProcessInfo {
        process_id: 42,
        parent_process_id: 0,
        exe_file: "Codex.exe".to_string(),
        executable_path: Some(std::path::PathBuf::from(
            r"D:\360Downloads\codexapp\app\Codex.exe",
        )),
    }];

    assert_eq!(find_codex_processes_from_snapshot(&processes), vec![42]);
}

#[cfg(windows)]
#[test]
fn find_codex_processes_ignores_lowercase_local_cli_binary() {
    let processes = [WindowsProcessInfo {
        process_id: 43,
        parent_process_id: 0,
        exe_file: "codex.exe".to_string(),
        executable_path: Some(std::path::PathBuf::from(
            r"D:\360Downloads\codexapp\app\codex.exe",
        )),
    }];

    assert!(find_codex_processes_from_snapshot(&processes).is_empty());
}

#[cfg(windows)]
#[test]
fn find_codex_processes_ignores_npm_cli_binary() {
    let processes = [WindowsProcessInfo {
        process_id: 44,
        parent_process_id: 0,
        exe_file: "codex.exe".to_string(),
        executable_path: Some(std::path::PathBuf::from(
            r"C:\Users\me\AppData\Roaming\npm\node_modules\@openai\codex\node_modules\@openai\codex-win32-x64\vendor\x86_64-pc-windows-msvc\bin\codex.exe",
        )),
    }];

    assert!(find_codex_processes_from_snapshot(&processes).is_empty());
}

#[cfg(windows)]
#[test]
fn find_codex_processes_ignores_packaged_resource_cli_binary() {
    let processes = [WindowsProcessInfo {
        process_id: 45,
        parent_process_id: 0,
        exe_file: "codex.exe".to_string(),
        executable_path: Some(std::path::PathBuf::from(
            r"C:\Program Files\WindowsApps\OpenAI.Codex_1.0.0.0_x64__abc\app\resources\codex.exe",
        )),
    }];

    assert!(find_codex_processes_from_snapshot(&processes).is_empty());
}

#[cfg(windows)]
#[test]
fn find_codex_processes_combines_store_and_local_installs() {
    let processes = [
        WindowsProcessInfo {
            process_id: 11,
            parent_process_id: 0,
            exe_file: "ChatGPT.exe".to_string(),
            executable_path: Some(std::path::PathBuf::from(
                r"C:\Program Files\WindowsApps\OpenAI.ChatGPT-Desktop_1.2026.133.0_x64__abc\app\ChatGPT.exe",
            )),
        },
        WindowsProcessInfo {
            process_id: 42,
            parent_process_id: 0,
            exe_file: "Codex.exe".to_string(),
            executable_path: Some(std::path::PathBuf::from(
                r"D:\360Downloads\codexapp\app\Codex.exe",
            )),
        },
    ];

    assert_eq!(find_codex_processes_from_snapshot(&processes), vec![11, 42]);
}

#[cfg(windows)]
#[test]
fn find_codex_processes_ignores_unrelated_processes() {
    let processes = [
        WindowsProcessInfo {
            process_id: 10,
            parent_process_id: 0,
            exe_file: "notepad.exe".to_string(),
            executable_path: Some(std::path::PathBuf::from(r"C:\Windows\notepad.exe")),
        },
        WindowsProcessInfo {
            process_id: 20,
            parent_process_id: 0,
            exe_file: "codex-plus-plus.exe".to_string(),
            executable_path: Some(std::path::PathBuf::from(
                r"D:\Programs\Codex++\codex-plus-plus.exe",
            )),
        },
    ];

    assert!(find_codex_processes_from_snapshot(&processes).is_empty());
}
