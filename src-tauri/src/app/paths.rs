use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};

const SMOKE_RUN_ENV: &str = "CAM_PACKAGED_SMOKE_RUN";
const SMOKE_DATA_DIR_ENV: &str = "CAM_PACKAGED_SMOKE_DATA_DIR";
const SMOKE_DATA_DIR_PREFIX: &str = "osir-codex-manager-smoke-";
const NEW_PROJECT_QUALIFIER: &str = "com.osir";
const NEW_PROJECT_ORGANIZATION: &str = "OSIR";
const NEW_PROJECT_APPLICATION: &str = "CodexManager";
const LEGACY_PROJECT_QUALIFIER: &str = "io.github";
const LEGACY_PROJECT_ORGANIZATION: &str = "wangnov"; // ownership-audit: allow-legacy
const LEGACY_PROJECT_APPLICATION: &str = "codexappmanager"; // ownership-audit: allow-legacy

#[derive(Debug, PartialEq, Eq)]
enum SmokeDataDir {
    Absent,
    Valid { run_id: String, path: PathBuf },
    Invalid,
}

fn valid_smoke_run_id(run_id: &str) -> bool {
    !run_id.is_empty()
        && run_id.len() <= 64
        && run_id
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_'))
}

fn validate_smoke_data_dir(
    run_id: Option<OsString>,
    requested: Option<OsString>,
    temp_dir: &Path,
) -> SmokeDataDir {
    let (run_id, requested) = match (run_id, requested) {
        (None, None) => return SmokeDataDir::Absent,
        (Some(run_id), Some(requested)) => (run_id, requested),
        (None, Some(_)) | (Some(_), None) => return SmokeDataDir::Invalid,
    };
    let Some(run_id) = run_id.to_str() else {
        return SmokeDataDir::Invalid;
    };
    if !valid_smoke_run_id(run_id) {
        return SmokeDataDir::Invalid;
    }

    let requested = PathBuf::from(requested);
    let expected_leaf = format!("{SMOKE_DATA_DIR_PREFIX}{run_id}");
    if !requested.is_absolute() || requested.file_name() != Some(OsStr::new(&expected_leaf)) {
        return SmokeDataDir::Invalid;
    }

    let Ok(metadata) = std::fs::symlink_metadata(&requested) else {
        return SmokeDataDir::Invalid;
    };
    if !metadata.is_dir() || metadata.file_type().is_symlink() {
        return SmokeDataDir::Invalid;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if metadata.permissions().mode() & 0o077 != 0 {
            return SmokeDataDir::Invalid;
        }
    }

    let (Ok(temp_dir), Ok(requested)) = (temp_dir.canonicalize(), requested.canonicalize()) else {
        return SmokeDataDir::Invalid;
    };
    if requested.parent() != Some(temp_dir.as_path()) {
        return SmokeDataDir::Invalid;
    }

    SmokeDataDir::Valid {
        run_id: run_id.to_string(),
        path: requested,
    }
}

fn smoke_data_dir_from_env() -> SmokeDataDir {
    validate_smoke_data_dir(
        std::env::var_os(SMOKE_RUN_ENV),
        std::env::var_os(SMOKE_DATA_DIR_ENV),
        &std::env::temp_dir(),
    )
}

fn select_data_dir(smoke: SmokeDataDir, production: Option<PathBuf>) -> Option<PathBuf> {
    match smoke {
        SmokeDataDir::Valid { path, .. } => Some(path),
        // If either smoke marker is present but the pair is invalid, fail
        // closed. Falling back to the production directory would let a broken
        // smoke harness read settings or run recovery against real user data.
        SmokeDataDir::Invalid => None,
        SmokeDataDir::Absent => production,
    }
}

fn select_staging_root(smoke: SmokeDataDir, temp_dir: &Path, process_id: u32) -> PathBuf {
    match smoke {
        SmokeDataDir::Valid { path, .. } => path.join("staging"),
        SmokeDataDir::Invalid => temp_dir
            .join(format!("osir-codex-manager-smoke-invalid-{process_id}"))
            .join("staging"),
        SmokeDataDir::Absent => temp_dir.join("osir-codex-manager").join("staging"),
    }
}

fn new_project_dirs() -> Option<directories::ProjectDirs> {
    directories::ProjectDirs::from(
        NEW_PROJECT_QUALIFIER,
        NEW_PROJECT_ORGANIZATION,
        NEW_PROJECT_APPLICATION,
    )
}

fn legacy_project_dirs() -> Option<directories::ProjectDirs> {
    directories::ProjectDirs::from(
        LEGACY_PROJECT_QUALIFIER,
        LEGACY_PROJECT_ORGANIZATION,
        LEGACY_PROJECT_APPLICATION,
    )
}

/// Move the old manager data directory into the OSIR-owned location exactly once.
///
/// A rename is used instead of copying so settings, themes, provenance, and any
/// future files move as one unit. If the new directory already exists, it wins;
/// the legacy directory is left untouched so a user can recover it manually.
fn migrate_legacy_dir(legacy: &Path, current: &Path) -> PathBuf {
    if legacy == current || !legacy.exists() || current.exists() {
        return current.to_path_buf();
    }
    let Ok(metadata) = std::fs::symlink_metadata(legacy) else {
        return current.to_path_buf();
    };
    if !metadata.is_dir() || metadata.file_type().is_symlink() {
        log::warn!("ignoring unsafe legacy manager data path: {}", legacy.display());
        return current.to_path_buf();
    }
    if let Some(parent) = current.parent() {
        if let Err(error) = std::fs::create_dir_all(parent) {
            log::warn!("failed to prepare Codex Manager data path: {error}");
            return current.to_path_buf();
        }
    }
    match std::fs::rename(legacy, current) {
        Ok(()) => log::info!("migrated legacy manager data to {}", current.display()),
        Err(error) => log::warn!("failed to migrate legacy manager data: {error}"),
    }
    current.to_path_buf()
}

fn production_data_dir() -> Option<PathBuf> {
    let current = new_project_dirs()?.data_dir().to_path_buf();
    let legacy = legacy_project_dirs()?.data_dir().to_path_buf();
    Some(migrate_legacy_dir(&legacy, &current))
}

/// Manager data directory shared by settings, provenance, and operation locks.
pub fn data_dir() -> Option<PathBuf> {
    let smoke = smoke_data_dir_from_env();
    let production = if matches!(&smoke, SmokeDataDir::Absent) {
        production_data_dir()
    } else {
        None
    };
    select_data_dir(smoke, production)
}

/// Manager cache directory for re-downloadable, non-critical content (currently
/// catalog preview thumbnails). Safe to clear at any time — distinct from
/// `data_dir`, which holds settings/provenance that must persist.
pub fn cache_dir() -> Option<PathBuf> {
    new_project_dirs()
        .map(|dirs| dirs.cache_dir().to_path_buf())
}

pub fn packaged_smoke_run_id() -> Option<String> {
    match smoke_data_dir_from_env() {
        SmokeDataDir::Valid { run_id, .. } => Some(run_id),
        SmokeDataDir::Absent | SmokeDataDir::Invalid => None,
    }
}

pub fn staging_root() -> PathBuf {
    let temp_dir = std::env::temp_dir();
    select_staging_root(smoke_data_dir_from_env(), &temp_dir, std::process::id())
}

pub fn settings_path() -> Option<PathBuf> {
    data_dir().map(|dir| dir.join("settings.json"))
}

pub fn provenance_path() -> Option<PathBuf> {
    data_dir().map(|dir| dir.join("provenance.json"))
}

pub fn codex_home_dir() -> Option<PathBuf> {
    // Codex itself gives CODEX_HOME precedence over the conventional
    // ~/.codex directory. The manager must use the same location or it can
    // appear to save a provider successfully while the launched Desktop app
    // continues reading a different config.toml (a common Windows setup when
    // CLI and Desktop are configured separately).
    if let Some(value) = std::env::var_os("CODEX_HOME") {
        let path = PathBuf::from(value);
        if path.is_absolute() {
            return Some(path);
        }
        log::warn!("ignoring relative CODEX_HOME; expected an absolute path");
    }
    directories::UserDirs::new().map(|dirs| dirs.home_dir().join(".codex"))
}

/// Default Codex-skin store. macOS keeps it beside the manager's data
/// (Application Support is the platform-correct home for app-managed
/// content); Windows uses the LOCAL app-data root instead of the roaming
/// one — skins are megabytes of re-downloadable content that must not ride
/// a domain roaming profile.
pub fn default_skins_store_dir() -> Option<PathBuf> {
    let dirs = new_project_dirs()?;
    if cfg!(target_os = "windows") {
        Some(dirs.data_local_dir().join("themes"))
    } else {
        Some(dirs.data_dir().join("themes"))
    }
}

#[cfg(test)]
mod tests {
    use super::{
        migrate_legacy_dir, select_data_dir, select_staging_root, validate_smoke_data_dir,
        SmokeDataDir, SMOKE_DATA_DIR_PREFIX,
    };

    fn test_run_id() -> String {
        format!("test-{}", uuid::Uuid::new_v4())
    }

    #[test]
    fn smoke_data_dir_requires_both_markers_and_an_exact_private_temp_child() {
        let temp_dir = std::env::temp_dir().canonicalize().unwrap();
        let run_id = test_run_id();
        let path = temp_dir.join(format!("{SMOKE_DATA_DIR_PREFIX}{run_id}"));
        std::fs::create_dir(&path).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700)).unwrap();
        }

        assert_eq!(
            validate_smoke_data_dir(
                Some(run_id.clone().into()),
                Some(path.clone().into_os_string()),
                &temp_dir,
            ),
            SmokeDataDir::Valid {
                run_id: run_id.clone(),
                path: path.canonicalize().unwrap(),
            }
        );
        assert_eq!(
            validate_smoke_data_dir(Some(run_id.clone().into()), None, &temp_dir),
            SmokeDataDir::Invalid
        );
        assert_eq!(
            validate_smoke_data_dir(None, Some(path.clone().into_os_string()), &temp_dir),
            SmokeDataDir::Invalid
        );
        assert_eq!(
            validate_smoke_data_dir(None, None, &temp_dir),
            SmokeDataDir::Absent
        );
        assert_eq!(
            validate_smoke_data_dir(
                Some("../invalid".into()),
                Some(path.clone().into_os_string()),
                &temp_dir,
            ),
            SmokeDataDir::Invalid
        );
        assert_eq!(
            validate_smoke_data_dir(
                Some(run_id.into()),
                Some(temp_dir.join("wrong-leaf").into_os_string()),
                &temp_dir,
            ),
            SmokeDataDir::Invalid
        );

        std::fs::remove_dir(&path).unwrap();
    }

    #[test]
    fn invalid_smoke_override_never_falls_back_to_production_data() {
        let production = std::env::temp_dir().join("real-manager-data");
        assert_eq!(
            select_data_dir(SmokeDataDir::Invalid, Some(production.clone())),
            None
        );
        assert_eq!(
            select_data_dir(SmokeDataDir::Absent, Some(production.clone())),
            Some(production)
        );

        let temp_dir = std::env::temp_dir();
        let production_staging = temp_dir.join("osir-codex-manager").join("staging");
        let invalid_staging = select_staging_root(SmokeDataDir::Invalid, &temp_dir, 1234);
        assert_ne!(invalid_staging, production_staging);
        assert_eq!(
            invalid_staging,
            temp_dir
                .join("osir-codex-manager-smoke-invalid-1234")
                .join("staging")
        );
    }

    #[test]
    fn legacy_data_is_migrated_once_without_overwriting_osir_data() {
        let root = std::env::temp_dir().join(format!("osir-path-migration-{}", uuid::Uuid::new_v4()));
        let legacy = root.join("legacy");
        let current = root.join("current");
        std::fs::create_dir_all(&legacy).unwrap();
        std::fs::write(legacy.join("settings.json"), b"legacy").unwrap();

        assert_eq!(migrate_legacy_dir(&legacy, &current), current);
        assert!(!legacy.exists());
        assert_eq!(std::fs::read_to_string(current.join("settings.json")).unwrap(), "legacy");

        std::fs::create_dir_all(&legacy).unwrap();
        std::fs::write(legacy.join("settings.json"), b"newer-legacy").unwrap();
        assert_eq!(migrate_legacy_dir(&legacy, &current), current);
        assert!(legacy.exists());
        assert_eq!(std::fs::read_to_string(current.join("settings.json")).unwrap(), "legacy");

        std::fs::remove_dir_all(root).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn smoke_data_dir_rejects_symlinks_and_group_access() {
        use std::os::unix::fs::{symlink, PermissionsExt};

        let temp_dir = std::env::temp_dir().canonicalize().unwrap();
        let target_run = test_run_id();
        let target = temp_dir.join(format!("{SMOKE_DATA_DIR_PREFIX}{target_run}"));
        std::fs::create_dir(&target).unwrap();
        std::fs::set_permissions(&target, std::fs::Permissions::from_mode(0o750)).unwrap();
        assert_eq!(
            validate_smoke_data_dir(
                Some(target_run.into()),
                Some(target.clone().into_os_string()),
                &temp_dir,
            ),
            SmokeDataDir::Invalid
        );

        let link_run = test_run_id();
        let link = temp_dir.join(format!("{SMOKE_DATA_DIR_PREFIX}{link_run}"));
        symlink(&target, &link).unwrap();
        assert_eq!(
            validate_smoke_data_dir(
                Some(link_run.into()),
                Some(link.clone().into_os_string()),
                &temp_dir,
            ),
            SmokeDataDir::Invalid
        );

        std::fs::remove_file(link).unwrap();
        std::fs::remove_dir(target).unwrap();
    }
}
