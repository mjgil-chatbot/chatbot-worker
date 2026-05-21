use super::*;
use ai_providers::sandbox::RunscMountTarget;
use std::path::PathBuf;

#[test]
fn gemini_runsc_mounts_prefer_legacy_home_when_shared_root_lacks_registry() {
    let _lock = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let tmp = tempfile::tempdir().unwrap();
    let previous_home = std::env::var_os("HOME");
    std::env::set_var("HOME", tmp.path());

    let legacy_gemini_dir = tmp.path().join(".gemini");
    let shared_gemini_dir = tmp.path().join(".rust-chatbot").join(".gemini");
    std::fs::create_dir_all(&legacy_gemini_dir).unwrap();
    std::fs::create_dir_all(&shared_gemini_dir).unwrap();
    std::fs::write(shared_gemini_dir.join("settings.local.json"), "{}\n").unwrap();
    std::fs::write(
        legacy_gemini_dir.join("projects.json"),
        "{\"projects\":{}}\n",
    )
    .unwrap();

    let overrides = ProviderRequestOverrides {
        gemini_settings_path: Some(shared_gemini_dir.join("settings.local.json")),
        ..Default::default()
    };

    let mounts = gemini_runsc_mounts(None, Some(&overrides));
    let home_mount = mounts
        .iter()
        .find(|mount| mount.target == RunscMountTarget::ContainerHome(PathBuf::from(".gemini")))
        .expect("expected writable .gemini home mount");

    assert_ne!(home_mount.host, legacy_gemini_dir);
    assert_eq!(
        std::fs::read_to_string(home_mount.host.join("projects.json")).unwrap(),
        "{\"projects\":{}}\n"
    );
    assert!(mounts.iter().any(|mount| {
        mount.host == shared_gemini_dir.join("settings.local.json")
            && mount.target == RunscMountTarget::SamePath
    }));
    assert!(mounts.iter().any(|mount| {
        mount.host == shared_gemini_dir.join("settings.local.json")
            && mount.target
                == RunscMountTarget::ContainerHome(PathBuf::from(".gemini/settings.json"))
    }));

    match previous_home {
        Some(value) => std::env::set_var("HOME", value),
        None => std::env::remove_var("HOME"),
    }
}

#[test]
fn gemini_runsc_mounts_use_shared_root_when_no_registry_exists_yet() {
    let _lock = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let tmp = tempfile::tempdir().unwrap();
    let previous_home = std::env::var_os("HOME");
    std::env::set_var("HOME", tmp.path());

    let shared_gemini_dir = tmp.path().join(".rust-chatbot").join(".gemini");
    std::fs::create_dir_all(&shared_gemini_dir).unwrap();
    let settings_path = shared_gemini_dir.join("settings.json");
    std::fs::write(&settings_path, "{}\n").unwrap();
    let sidecar_path = gemini_coordinator_sidecar_path(&settings_path);
    std::fs::write(&sidecar_path, "{}\n").unwrap();

    let overrides = ProviderRequestOverrides {
        gemini_settings_path: Some(settings_path.clone()),
        ..Default::default()
    };

    let mounts = gemini_runsc_mounts(None, Some(&overrides));
    let home_mount = mounts
        .iter()
        .find(|mount| mount.target == RunscMountTarget::ContainerHome(PathBuf::from(".gemini")))
        .expect("expected writable .gemini home mount");

    assert_ne!(home_mount.host, shared_gemini_dir);
    assert!(home_mount.host.is_dir());
    assert!(mounts
        .iter()
        .any(|mount| { mount.host == sidecar_path && mount.target == RunscMountTarget::SamePath }));

    match previous_home {
        Some(value) => std::env::set_var("HOME", value),
        None => std::env::remove_var("HOME"),
    }
}
