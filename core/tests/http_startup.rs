use std::{fs, process::Command};

#[test]
fn enabled_http_rejects_missing_or_invalid_secret_before_database_startup_without_leaking_values() {
    let directory = std::env::temp_dir().join(format!("bc-http-startup-{}", std::process::id()));
    fs::create_dir_all(&directory).unwrap();
    let manifest = directory.join("manifest.yaml");
    fs::write(&manifest, "release: 0.1.0\ndeployment_mode: self_hosted\nmodules: {}\nhttp:\n  public_origin: http://127.0.0.1:3000\n  admin_token: { file: ./admin-token }\n").unwrap();
    for token in [None, Some("SECRET_VALUE_MUST_NOT_BE_EXPOSED")] {
        if let Some(value) = token {
            fs::write(directory.join("admin-token"), value).unwrap();
        }
        let output = Command::new(env!("CARGO_BIN_EXE_better-commerce-core"))
            .env("MANIFEST_PATH", &manifest)
            .env_remove("BC_RESOLVED_ADMIN_TOKEN")
            .env_remove("READINESS_DATABASE_URL")
            .output()
            .unwrap();
        assert!(!output.status.success());
        let diagnostics = String::from_utf8_lossy(&output.stderr);
        assert!(diagnostics.contains("HttpConfigurationError"));
        assert!(!diagnostics.contains("SECRET_VALUE_MUST_NOT_BE_EXPOSED"));
        assert!(!diagnostics.contains("READINESS_DATABASE_URL"));
    }
    fs::remove_file(directory.join("admin-token")).unwrap();
    fs::remove_file(manifest).unwrap();
    fs::remove_dir(directory).unwrap();
}
