use std::process::Command;

fn run(args: &[&str], arl: Option<&str>) -> std::process::Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_melimo"));
    command
        .args(args)
        .env_remove("DEEZER_ARL")
        .env("MELIMO_NO_STORE", "1");
    if let Some(arl) = arl {
        command.env("DEEZER_ARL", arl);
    }
    command.output().unwrap()
}

#[test]
fn check_auth_does_not_require_a_terminal_or_echo_bad_credentials() {
    for credential in [None, Some("SYNTHETIC_PRIVATE_COOKIE")] {
        let output = run(&["--check-auth"], credential);
        assert!(!output.status.success());
        let error = String::from_utf8(output.stderr).unwrap();
        assert!(error.contains("DEEZER_ARL"));
        assert!(!error.contains("interactive terminal"));
        assert!(!error.contains("SYNTHETIC_PRIVATE_COOKIE"));
        assert!(output.stdout.is_empty());
    }
}

#[test]
fn help_is_headless_and_conflicting_options_are_rejected() {
    let output = run(&["--help"], None);
    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("--check-auth"));
    assert!(stdout.contains("--version"));
    let output = run(&["--mock", "--check-auth"], None);
    assert!(!output.status.success());
    let output = run(&["SYNTHETIC_PRIVATE_COOKIE"], None);
    assert!(
        !String::from_utf8(output.stderr)
            .unwrap()
            .contains("SYNTHETIC_PRIVATE_COOKIE")
    );
}

#[test]
fn version_is_headless() {
    let output = run(&["--version"], None);
    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains(concat!("Mélimo ", env!("CARGO_PKG_VERSION"))));
}
