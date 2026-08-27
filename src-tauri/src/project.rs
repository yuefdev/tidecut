use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{HashMap, HashSet};
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};
use tauri::{AppHandle, Manager};

const FILE_FORMAT: &str = "astral-lunar-project";
const FILE_FORMAT_VERSION: u32 = 1;
const RECOVERY_EXTENSION: &str = "autosave";
// The editor has one unsaved workspace. Keep its recovery snapshot stable
// across launches instead of using the randomly generated document id.
const UNSAVED_RECOVERY_SNAPSHOT_ID: &str = "unsaved-project";
const DEFAULT_RELINK_DEPTH: u8 = 8;
const MAX_RELINK_DEPTH: u8 = 32;
const MAX_RELINK_FILES: usize = 100_000;
const QUICK_HASH_SAMPLE_BYTES: usize = 64 * 1024;
static PROJECT_WRITE_LOCK: Mutex<()> = Mutex::new(());

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectCommandError {
    pub code: String,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
}

impl ProjectCommandError {
    fn new(code: &str, message: impl Into<String>, path: Option<&Path>) -> Self {
        Self {
            code: code.to_string(),
            message: message.into(),
            path: path.map(path_to_string),
        }
    }

    fn io(code: &str, error: std::io::Error, path: &Path) -> Self {
        Self::new(code, error.to_string(), Some(path))
    }
}

impl std::fmt::Display for ProjectCommandError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{}: {}", self.code, self.message)
    }
}

impl std::error::Error for ProjectCommandError {}

type CommandResult<T> = Result<T, ProjectCommandError>;

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SaveProjectRequest {
    pub path: String,
    pub document: Value,
    #[serde(default)]
    pub project_id: Option<String>,
    #[serde(default)]
    pub expected_checksum: Option<String>,
    #[serde(default = "default_true")]
    pub create_backup: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SaveProjectResult {
    pub path: String,
    pub checksum: String,
    pub bytes_written: u64,
    pub saved_at_ms: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub backup_path: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LoadProjectResult {
    pub path: String,
    pub document: Value,
    pub checksum: String,
    pub saved_at_ms: u64,
    pub verified: bool,
    pub recovered_from_backup: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub original_path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub base_checksum: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub recovery_snapshot_id: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RecoverySnapshotRequest {
    pub project_id: String,
    pub document: Value,
    #[serde(default)]
    pub original_path: Option<String>,
    #[serde(default)]
    pub base_checksum: Option<String>,
    #[serde(default)]
    pub recovered_from_backup: bool,
    #[serde(default)]
    pub reason: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RecoverySnapshotInfo {
    pub snapshot_id: String,
    pub project_id: String,
    pub checksum: String,
    pub updated_at_ms: u64,
    pub bytes_written: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub original_path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    pub is_valid: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RecoveryMetadata {
    snapshot_id: String,
    project_id: String,
    #[serde(default)]
    original_path: Option<String>,
    #[serde(default)]
    base_checksum: Option<String>,
    #[serde(default)]
    recovered_from_backup: bool,
    #[serde(default)]
    reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct StoredDocument {
    file_format: String,
    format_version: u32,
    kind: String,
    saved_at_ms: u64,
    checksum: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    recovery: Option<RecoveryMetadata>,
    document: Value,
}

#[derive(Debug)]
struct DecodedDocument {
    kind: String,
    document: Value,
    checksum: String,
    saved_at_ms: u64,
    verified: bool,
    recovery: Option<RecoveryMetadata>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MediaReference {
    #[serde(default)]
    pub asset_id: Option<String>,
    pub path: String,
    #[serde(default)]
    pub size_bytes: Option<u64>,
    #[serde(default)]
    pub quick_hash: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MediaInspection {
    pub path: String,
    pub file_name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub extension: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kind: Option<String>,
    pub exists: bool,
    pub supported: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub size_bytes: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub modified_at_ms: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub quick_hash: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MediaPathStatus {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub asset_id: Option<String>,
    pub path: String,
    pub exists: bool,
    pub is_file: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub size_bytes: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub modified_at_ms: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RelinkMediaRequest {
    pub missing: Vec<MediaReference>,
    pub search_roots: Vec<String>,
    #[serde(default)]
    pub max_depth: Option<u8>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RelinkCandidate {
    pub path: String,
    pub size_bytes: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub quick_hash: Option<String>,
    pub confidence: u8,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RelinkResolution {
    pub reference: MediaReference,
    pub replacement_path: String,
    pub confidence: u8,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AmbiguousRelink {
    pub reference: MediaReference,
    pub candidates: Vec<RelinkCandidate>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RelinkMediaResult {
    pub resolved: Vec<RelinkResolution>,
    pub ambiguous: Vec<AmbiguousRelink>,
    pub unresolved: Vec<MediaReference>,
    pub scanned_files: usize,
    pub truncated: bool,
}

#[derive(Debug, Clone)]
struct CandidateFile {
    path: PathBuf,
    file_name_lower: String,
    size_bytes: u64,
}

#[tauri::command]
pub fn save_project(
    app: AppHandle,
    request: SaveProjectRequest,
) -> CommandResult<SaveProjectResult> {
    let project_id = request.project_id.clone();
    let result = save_project_impl(&request)?;

    if let Some(project_id) = project_id {
        if let Ok(snapshot_id) = snapshot_id_for_project(&project_id) {
            if let Ok(directory) = recovery_directory(&app) {
                let _ = fs::remove_file(recovery_path(&directory, &snapshot_id));
                let _ = fs::remove_file(recovery_path(&directory, UNSAVED_RECOVERY_SNAPSHOT_ID));
            }
        }
    }

    Ok(result)
}

#[tauri::command]
pub fn load_project(path: String) -> CommandResult<LoadProjectResult> {
    load_project_impl(Path::new(&path))
}

#[tauri::command]
pub fn write_recovery_snapshot(
    app: AppHandle,
    request: RecoverySnapshotRequest,
) -> CommandResult<RecoverySnapshotInfo> {
    let directory = recovery_directory(&app)?;
    write_recovery_snapshot_impl(&directory, &request)
}

#[tauri::command]
pub fn list_recovery_snapshots(app: AppHandle) -> CommandResult<Vec<RecoverySnapshotInfo>> {
    let directory = recovery_directory(&app)?;
    list_recovery_snapshots_impl(&directory)
}

#[tauri::command]
pub fn load_recovery_snapshot(
    app: AppHandle,
    snapshot_id: String,
) -> CommandResult<LoadProjectResult> {
    let directory = recovery_directory(&app)?;
    load_recovery_snapshot_impl(&directory, &snapshot_id)
}

fn load_recovery_snapshot_impl(
    directory: &Path,
    snapshot_id: &str,
) -> CommandResult<LoadProjectResult> {
    validate_snapshot_id(snapshot_id)?;
    let path = recovery_path(directory, snapshot_id);
    let decoded = read_decoded_document(&path)?;
    let recovery = decoded.recovery.as_ref().ok_or_else(|| {
        ProjectCommandError::new(
            "recovery_metadata_missing",
            "Recovery snapshot does not contain recovery metadata",
            Some(&path),
        )
    })?;
    if decoded.kind != "recovery" || recovery.snapshot_id != snapshot_id {
        return Err(ProjectCommandError::new(
            "recovery_metadata_invalid",
            "Recovery snapshot identity does not match its file name",
            Some(&path),
        ));
    }
    let original_path = recovery.original_path.clone();
    let base_checksum = recovery.base_checksum.clone();
    let recovered_from_backup = recovery.recovered_from_backup;
    Ok(LoadProjectResult {
        path: path_to_string(&path),
        document: decoded.document,
        checksum: decoded.checksum,
        saved_at_ms: decoded.saved_at_ms,
        verified: decoded.verified,
        recovered_from_backup,
        original_path,
        base_checksum,
        recovery_snapshot_id: Some(snapshot_id.to_string()),
    })
}

#[tauri::command]
pub fn discard_recovery_snapshot(app: AppHandle, snapshot_id: String) -> CommandResult<()> {
    validate_snapshot_id(&snapshot_id)?;
    let directory = recovery_directory(&app)?;
    let path = recovery_path(&directory, &snapshot_id);
    match fs::remove_file(&path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(ProjectCommandError::io(
            "recovery_delete_failed",
            error,
            &path,
        )),
    }
}

#[tauri::command]
pub fn inspect_media(paths: Vec<String>) -> Vec<MediaInspection> {
    inspect_media_impl(paths)
}

#[tauri::command]
pub fn find_missing_media(references: Vec<MediaReference>) -> Vec<MediaPathStatus> {
    references
        .into_iter()
        .map(|reference| inspect_media_reference(&reference))
        .collect()
}

#[tauri::command]
pub fn relink_media(request: RelinkMediaRequest) -> CommandResult<RelinkMediaResult> {
    relink_media_impl(&request)
}

fn save_project_impl(request: &SaveProjectRequest) -> CommandResult<SaveProjectResult> {
    let _write_guard = PROJECT_WRITE_LOCK.lock().map_err(|_| {
        ProjectCommandError::new(
            "project_write_lock_failed",
            "Project writer is unavailable after an internal failure",
            None,
        )
    })?;
    validate_document(&request.document)?;
    let path = absolute_path(Path::new(&request.path))?;

    if path.is_dir() {
        return Err(ProjectCommandError::new(
            "invalid_project_path",
            "Project path points to a directory",
            Some(&path),
        ));
    }

    if let Some(expected) = request.expected_checksum.as_deref() {
        if !path.exists() {
            return Err(ProjectCommandError::new(
                "save_conflict",
                "Project was removed or moved after it was opened",
                Some(&path),
            ));
        }
        let current = read_decoded_document(&path)?;
        if !checksum_equal(expected, &current.checksum) {
            return Err(ProjectCommandError::new(
                "save_conflict",
                "Project changed on disk; refusing to overwrite a newer revision",
                Some(&path),
            ));
        }
    }

    let saved_at_ms = unix_time_ms();
    let checksum = checksum_document(&request.document)?;
    let stored = StoredDocument {
        file_format: FILE_FORMAT.to_string(),
        format_version: FILE_FORMAT_VERSION,
        kind: "project".to_string(),
        saved_at_ms,
        checksum: checksum.clone(),
        recovery: None,
        document: request.document.clone(),
    };
    let bytes = serialize_stored_document(&stored)?;

    let backup_path = if request.create_backup && path.is_file() {
        let backup = backup_path(&path);
        let existing = fs::read(&path)
            .map_err(|error| ProjectCommandError::io("backup_read_failed", error, &path))?;
        atomic_write_bytes(&backup, &existing)?;
        Some(path_to_string(&backup))
    } else {
        None
    };

    atomic_write_bytes(&path, &bytes)?;

    Ok(SaveProjectResult {
        path: path_to_string(&path),
        checksum,
        bytes_written: bytes.len() as u64,
        saved_at_ms,
        backup_path,
    })
}

fn load_project_impl(path: &Path) -> CommandResult<LoadProjectResult> {
    let path = absolute_path(path)?;
    match load_document_at_path(&path, false) {
        Ok(result) => Ok(result),
        Err(primary_error) => {
            if !matches!(
                primary_error.code.as_str(),
                "project_read_failed"
                    | "project_parse_failed"
                    | "project_integrity_failed"
                    | "invalid_project_document"
            ) {
                return Err(primary_error);
            }
            let backup = backup_path(&path);
            if !backup.is_file() {
                return Err(primary_error);
            }

            match load_document_at_path(&backup, true) {
                Ok(mut result) => {
                    result.path = path_to_string(&path);
                    result.recovered_from_backup = true;
                    Ok(result)
                }
                Err(_) => Err(primary_error),
            }
        }
    }
}

fn load_document_at_path(
    path: &Path,
    recovered_from_backup: bool,
) -> CommandResult<LoadProjectResult> {
    let decoded = read_decoded_document(path)?;
    Ok(LoadProjectResult {
        path: path_to_string(path),
        document: decoded.document,
        checksum: decoded.checksum,
        saved_at_ms: decoded.saved_at_ms,
        verified: decoded.verified,
        recovered_from_backup,
        original_path: None,
        base_checksum: None,
        recovery_snapshot_id: None,
    })
}

fn read_decoded_document(path: &Path) -> CommandResult<DecodedDocument> {
    let bytes = fs::read(path)
        .map_err(|error| ProjectCommandError::io("project_read_failed", error, path))?;
    let value: Value = serde_json::from_slice(&bytes).map_err(|error| {
        ProjectCommandError::new("project_parse_failed", error.to_string(), Some(path))
    })?;

    if value.get("fileFormat").and_then(Value::as_str) == Some(FILE_FORMAT) {
        let stored: StoredDocument = serde_json::from_value(value).map_err(|error| {
            ProjectCommandError::new("project_parse_failed", error.to_string(), Some(path))
        })?;

        if stored.format_version > FILE_FORMAT_VERSION {
            return Err(ProjectCommandError::new(
                "project_version_unsupported",
                format!(
                    "Project format version {} is newer than supported version {}",
                    stored.format_version, FILE_FORMAT_VERSION
                ),
                Some(path),
            ));
        }

        let actual = checksum_document(&stored.document)?;
        if !checksum_equal(&stored.checksum, &actual) {
            return Err(ProjectCommandError::new(
                "project_integrity_failed",
                "Project checksum does not match its contents",
                Some(path),
            ));
        }

        validate_document(&stored.document)?;
        return Ok(DecodedDocument {
            kind: stored.kind,
            document: stored.document,
            checksum: actual,
            saved_at_ms: stored.saved_at_ms,
            verified: true,
            recovery: stored.recovery,
        });
    }

    // Legacy raw JSON remains readable and is upgraded on the next save.
    validate_document(&value)?;
    let checksum = checksum_document(&value)?;
    let saved_at_ms = fs::metadata(path)
        .ok()
        .and_then(|metadata| metadata.modified().ok())
        .and_then(system_time_ms)
        .unwrap_or_default();
    Ok(DecodedDocument {
        kind: "legacy".to_string(),
        document: value,
        checksum,
        saved_at_ms,
        verified: false,
        recovery: None,
    })
}

fn write_recovery_snapshot_impl(
    directory: &Path,
    request: &RecoverySnapshotRequest,
) -> CommandResult<RecoverySnapshotInfo> {
    let _write_guard = PROJECT_WRITE_LOCK.lock().map_err(|_| {
        ProjectCommandError::new(
            "project_write_lock_failed",
            "Recovery writer is unavailable after an internal failure",
            None,
        )
    })?;
    validate_document(&request.document)?;
    let snapshot_id = recovery_snapshot_id(request)?;
    fs::create_dir_all(directory)
        .map_err(|error| ProjectCommandError::io("recovery_directory_failed", error, directory))?;

    if request.original_path.is_none() {
        remove_legacy_unsaved_snapshots(directory, &snapshot_id);
    }

    let path = recovery_path(directory, &snapshot_id);
    let saved_at_ms = unix_time_ms();
    let checksum = checksum_document(&request.document)?;
    let recovery = RecoveryMetadata {
        snapshot_id: snapshot_id.clone(),
        project_id: request.project_id.clone(),
        original_path: request.original_path.clone(),
        base_checksum: request.base_checksum.clone(),
        recovered_from_backup: request.recovered_from_backup,
        reason: request.reason.clone(),
    };
    let stored = StoredDocument {
        file_format: FILE_FORMAT.to_string(),
        format_version: FILE_FORMAT_VERSION,
        kind: "recovery".to_string(),
        saved_at_ms,
        checksum: checksum.clone(),
        recovery: Some(recovery),
        document: request.document.clone(),
    };
    let bytes = serialize_stored_document(&stored)?;
    atomic_write_bytes(&path, &bytes)?;

    Ok(RecoverySnapshotInfo {
        snapshot_id,
        project_id: request.project_id.clone(),
        checksum,
        updated_at_ms: saved_at_ms,
        bytes_written: bytes.len() as u64,
        original_path: request.original_path.clone(),
        reason: request.reason.clone(),
        is_valid: true,
        error: None,
    })
}

fn list_recovery_snapshots_impl(directory: &Path) -> CommandResult<Vec<RecoverySnapshotInfo>> {
    if !directory.exists() {
        return Ok(Vec::new());
    }

    let entries = fs::read_dir(directory)
        .map_err(|error| ProjectCommandError::io("recovery_list_failed", error, directory))?;
    let mut snapshots = Vec::new();

    for entry in entries {
        let entry = match entry {
            Ok(entry) => entry,
            Err(_) => continue,
        };
        let path = entry.path();
        if path.extension().and_then(|extension| extension.to_str()) != Some(RECOVERY_EXTENSION) {
            continue;
        }

        let snapshot_id = path
            .file_stem()
            .and_then(|name| name.to_str())
            .unwrap_or("unknown")
            .to_string();
        let bytes_written = entry
            .metadata()
            .map(|value| value.len())
            .unwrap_or_default();

        match read_decoded_document(&path) {
            Ok(decoded)
                if decoded.kind == "recovery"
                    && decoded
                        .recovery
                        .as_ref()
                        .is_some_and(|metadata| metadata.snapshot_id == snapshot_id) =>
            {
                let recovery = decoded.recovery.expect("recovery was checked above");
                snapshots.push(RecoverySnapshotInfo {
                    snapshot_id: recovery.snapshot_id,
                    project_id: recovery.project_id,
                    checksum: decoded.checksum,
                    updated_at_ms: decoded.saved_at_ms,
                    bytes_written,
                    original_path: recovery.original_path,
                    reason: recovery.reason,
                    is_valid: true,
                    error: None,
                });
            }
            Ok(_) => snapshots.push(RecoverySnapshotInfo {
                snapshot_id,
                project_id: "unknown".to_string(),
                checksum: String::new(),
                updated_at_ms: entry
                    .metadata()
                    .ok()
                    .and_then(|metadata| metadata.modified().ok())
                    .and_then(system_time_ms)
                    .unwrap_or_default(),
                bytes_written,
                original_path: None,
                reason: None,
                is_valid: false,
                error: Some("Recovery metadata is missing or invalid".to_string()),
            }),
            Err(error) => snapshots.push(RecoverySnapshotInfo {
                snapshot_id,
                project_id: "unknown".to_string(),
                checksum: String::new(),
                updated_at_ms: entry
                    .metadata()
                    .ok()
                    .and_then(|metadata| metadata.modified().ok())
                    .and_then(system_time_ms)
                    .unwrap_or_default(),
                bytes_written,
                original_path: None,
                reason: None,
                is_valid: false,
                error: Some(error.message),
            }),
        }
    }

    snapshots.sort_by_key(|snapshot| std::cmp::Reverse(snapshot.updated_at_ms));

    // Older builds used a fresh random project id for every unsaved launch.
    // Treat those pathless snapshots as one workspace and retain only the
    // newest one so they cannot keep reappearing in the recovery dialog.
    if let Some(latest_unsaved_id) = snapshots
        .iter()
        .filter(|snapshot| snapshot.is_valid && snapshot.original_path.is_none())
        .max_by_key(|snapshot| snapshot.updated_at_ms)
        .map(|snapshot| snapshot.snapshot_id.clone())
    {
        for snapshot in snapshots.iter().filter(|snapshot| {
            snapshot.is_valid
                && snapshot.original_path.is_none()
                && snapshot.snapshot_id != latest_unsaved_id
        }) {
            let _ = fs::remove_file(recovery_path(directory, &snapshot.snapshot_id));
        }
        snapshots.retain(|snapshot| {
            !snapshot.is_valid
                || snapshot.original_path.is_some()
                || snapshot.snapshot_id == latest_unsaved_id
        });
    }

    Ok(snapshots)
}

fn recovery_directory(app: &AppHandle) -> CommandResult<PathBuf> {
    app.path()
        .app_data_dir()
        .map(|path| path.join("recovery"))
        .map_err(|error| {
            ProjectCommandError::new("recovery_directory_failed", error.to_string(), None)
        })
}

fn snapshot_id_for_project(project_id: &str) -> CommandResult<String> {
    let trimmed = project_id.trim();
    if trimmed.is_empty() {
        return Err(ProjectCommandError::new(
            "invalid_project_id",
            "Project id cannot be empty",
            None,
        ));
    }

    let safe: String = trimmed
        .chars()
        .filter(|character| character.is_ascii_alphanumeric() || matches!(character, '-' | '_'))
        .take(80)
        .collect();
    if safe == trimmed && !safe.is_empty() {
        return Ok(safe);
    }

    Ok(format!("project-{}", &sha256_hex(trimmed.as_bytes())[..24]))
}

fn recovery_snapshot_id(request: &RecoverySnapshotRequest) -> CommandResult<String> {
    if request.original_path.is_none() {
        return Ok(UNSAVED_RECOVERY_SNAPSHOT_ID.to_string());
    }
    snapshot_id_for_project(&request.project_id)
}

fn remove_legacy_unsaved_snapshots(directory: &Path, keep_snapshot_id: &str) {
    let Ok(entries) = fs::read_dir(directory) else {
        return;
    };

    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|extension| extension.to_str()) != Some(RECOVERY_EXTENSION) {
            continue;
        }
        let snapshot_id = path
            .file_stem()
            .and_then(|name| name.to_str())
            .unwrap_or_default();
        if snapshot_id == keep_snapshot_id {
            continue;
        }

        let is_legacy_unsaved = read_decoded_document(&path)
            .ok()
            .and_then(|decoded| decoded.recovery)
            .is_some_and(|recovery| recovery.original_path.is_none());
        if is_legacy_unsaved {
            let _ = fs::remove_file(path);
        }
    }
}

fn validate_snapshot_id(snapshot_id: &str) -> CommandResult<()> {
    if snapshot_id.is_empty()
        || snapshot_id.len() > 88
        || !snapshot_id
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || matches!(character, '-' | '_'))
    {
        return Err(ProjectCommandError::new(
            "invalid_snapshot_id",
            "Recovery snapshot id contains invalid characters",
            None,
        ));
    }
    Ok(())
}

fn recovery_path(directory: &Path, snapshot_id: &str) -> PathBuf {
    directory.join(format!("{snapshot_id}.{RECOVERY_EXTENSION}"))
}

fn inspect_media_impl(paths: Vec<String>) -> Vec<MediaInspection> {
    let mut seen = HashSet::new();
    let mut results = Vec::new();

    for raw_path in paths {
        let path = PathBuf::from(raw_path.trim());
        let dedupe_key = normalized_path_key(&path);
        if !seen.insert(dedupe_key) {
            continue;
        }
        results.push(inspect_media_path(&path));
    }
    results
}

fn inspect_media_path(path: &Path) -> MediaInspection {
    let path_string = path_to_string(path);
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or_default()
        .to_string();
    let extension = normalized_extension(path);
    let kind = extension
        .as_deref()
        .and_then(media_kind)
        .map(str::to_string);
    let supported = kind.is_some();

    let metadata = match fs::metadata(path) {
        Ok(metadata) if metadata.is_file() => metadata,
        Ok(_) => {
            return MediaInspection {
                path: path_string,
                file_name,
                extension,
                kind,
                exists: true,
                supported,
                size_bytes: None,
                modified_at_ms: None,
                quick_hash: None,
                error: Some("Path is not a regular file".to_string()),
            }
        }
        Err(error) => {
            return MediaInspection {
                path: path_string,
                file_name,
                extension,
                kind,
                exists: false,
                supported,
                size_bytes: None,
                modified_at_ms: None,
                quick_hash: None,
                error: Some(error.to_string()),
            }
        }
    };

    let quick_hash_result = quick_file_hash(path, metadata.len());
    let (quick_hash, hash_error) = match quick_hash_result {
        Ok(value) => (Some(value), None),
        Err(error) => (None, Some(error.to_string())),
    };

    MediaInspection {
        path: path_string,
        file_name,
        extension,
        kind,
        exists: true,
        supported,
        size_bytes: Some(metadata.len()),
        modified_at_ms: metadata.modified().ok().and_then(system_time_ms),
        quick_hash,
        error: if supported {
            hash_error
        } else {
            Some("Unsupported media format".to_string())
        },
    }
}

fn inspect_media_reference(reference: &MediaReference) -> MediaPathStatus {
    let path = Path::new(&reference.path);
    match fs::metadata(path) {
        Ok(metadata) => MediaPathStatus {
            asset_id: reference.asset_id.clone(),
            path: reference.path.clone(),
            exists: true,
            is_file: metadata.is_file(),
            size_bytes: metadata.is_file().then_some(metadata.len()),
            modified_at_ms: metadata.modified().ok().and_then(system_time_ms),
            reason: (!metadata.is_file()).then(|| "Path is not a regular file".to_string()),
        },
        Err(error) => MediaPathStatus {
            asset_id: reference.asset_id.clone(),
            path: reference.path.clone(),
            exists: false,
            is_file: false,
            size_bytes: None,
            modified_at_ms: None,
            reason: Some(error.to_string()),
        },
    }
}

fn relink_media_impl(request: &RelinkMediaRequest) -> CommandResult<RelinkMediaResult> {
    if request.search_roots.is_empty() {
        return Ok(RelinkMediaResult {
            resolved: Vec::new(),
            ambiguous: Vec::new(),
            unresolved: request.missing.clone(),
            scanned_files: 0,
            truncated: false,
        });
    }

    let max_depth = request
        .max_depth
        .unwrap_or(DEFAULT_RELINK_DEPTH)
        .min(MAX_RELINK_DEPTH);
    let (candidate_files, truncated) = collect_candidate_files(&request.search_roots, max_depth)?;
    let scanned_files = candidate_files.len();
    let mut by_name: HashMap<String, Vec<&CandidateFile>> = HashMap::new();
    let mut by_size: HashMap<u64, Vec<&CandidateFile>> = HashMap::new();
    for candidate in &candidate_files {
        by_name
            .entry(candidate.file_name_lower.clone())
            .or_default()
            .push(candidate);
        by_size
            .entry(candidate.size_bytes)
            .or_default()
            .push(candidate);
    }

    let mut hash_cache: HashMap<PathBuf, Option<String>> = HashMap::new();
    let mut resolved = Vec::new();
    let mut ambiguous = Vec::new();
    let mut unresolved = Vec::new();

    for reference in &request.missing {
        let file_name = Path::new(&reference.path)
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or_default()
            .to_lowercase();
        let mut candidates = by_name.get(&file_name).cloned().unwrap_or_default();
        if reference.quick_hash.is_some() {
            if let Some(expected_size) = reference.size_bytes {
                candidates.extend(by_size.get(&expected_size).cloned().unwrap_or_default());
            }
        }
        let mut seen_candidates = HashSet::new();
        candidates.retain(|candidate| seen_candidates.insert(candidate.path.clone()));
        if candidates.is_empty() {
            unresolved.push(reference.clone());
            continue;
        }

        let mut ranked = Vec::new();
        for candidate in candidates {
            let same_name = candidate.file_name_lower == file_name;
            if !same_name && (reference.quick_hash.is_none() || reference.size_bytes.is_none()) {
                continue;
            }
            let mut confidence = 60;
            let mut reason = "filename".to_string();

            if let Some(expected_size) = reference.size_bytes {
                if expected_size != candidate.size_bytes {
                    continue;
                }
                confidence = 80;
                reason = "filename-and-size".to_string();
            }

            let mut quick_hash = None;
            if let Some(expected_hash) = reference.quick_hash.as_deref() {
                let candidate_hash = hash_cache
                    .entry(candidate.path.clone())
                    .or_insert_with(|| quick_file_hash(&candidate.path, candidate.size_bytes).ok())
                    .clone();
                if candidate_hash
                    .as_deref()
                    .is_some_and(|value| checksum_equal(expected_hash, value))
                {
                    confidence = 100;
                    reason = if same_name {
                        "quick-hash".to_string()
                    } else {
                        "quick-hash-renamed".to_string()
                    };
                    quick_hash = candidate_hash;
                } else {
                    continue;
                }
            }

            ranked.push(RelinkCandidate {
                path: path_to_string(&candidate.path),
                size_bytes: candidate.size_bytes,
                quick_hash,
                confidence,
                reason,
            });
        }

        if ranked.is_empty() {
            unresolved.push(reference.clone());
            continue;
        }

        ranked.sort_by(|left, right| {
            right
                .confidence
                .cmp(&left.confidence)
                .then_with(|| left.path.cmp(&right.path))
        });
        let best_confidence = ranked[0].confidence;
        let best: Vec<_> = ranked
            .into_iter()
            .filter(|candidate| candidate.confidence == best_confidence)
            .collect();

        if best.len() == 1 {
            let candidate = &best[0];
            resolved.push(RelinkResolution {
                reference: reference.clone(),
                replacement_path: candidate.path.clone(),
                confidence: candidate.confidence,
                reason: candidate.reason.clone(),
            });
        } else {
            ambiguous.push(AmbiguousRelink {
                reference: reference.clone(),
                candidates: best,
            });
        }
    }

    Ok(RelinkMediaResult {
        resolved,
        ambiguous,
        unresolved,
        scanned_files,
        truncated,
    })
}

fn collect_candidate_files(
    roots: &[String],
    max_depth: u8,
) -> CommandResult<(Vec<CandidateFile>, bool)> {
    let mut files = Vec::new();
    let mut stack: Vec<(PathBuf, u8)> = roots.iter().map(|root| (PathBuf::from(root), 0)).collect();
    let mut visited = HashSet::new();
    let mut truncated = false;

    while let Some((path, depth)) = stack.pop() {
        if files.len() >= MAX_RELINK_FILES {
            truncated = true;
            break;
        }

        let metadata = match fs::symlink_metadata(&path) {
            Ok(metadata) => metadata,
            Err(_) => continue,
        };
        if metadata.file_type().is_symlink() {
            continue;
        }

        if metadata.is_file() {
            if normalized_extension(&path)
                .as_deref()
                .and_then(media_kind)
                .is_none()
            {
                continue;
            }
            files.push(CandidateFile {
                file_name_lower: path
                    .file_name()
                    .and_then(|name| name.to_str())
                    .unwrap_or_default()
                    .to_lowercase(),
                path,
                size_bytes: metadata.len(),
            });
            continue;
        }

        if !metadata.is_dir() || depth >= max_depth {
            continue;
        }
        let identity = fs::canonicalize(&path).unwrap_or(path.clone());
        if !visited.insert(normalized_path_key(&identity)) {
            continue;
        }

        let entries = match fs::read_dir(&path) {
            Ok(entries) => entries,
            Err(_) => continue,
        };
        for entry in entries.flatten() {
            stack.push((entry.path(), depth.saturating_add(1)));
        }
    }

    Ok((files, truncated))
}

fn media_kind(extension: &str) -> Option<&'static str> {
    match extension {
        "mp4" | "mov" | "m4v" | "mkv" | "avi" | "webm" | "mxf" | "ts" | "mts" | "m2ts" | "wmv"
        | "flv" | "ogv" => Some("video"),
        "wav" | "mp3" | "m4a" | "aac" | "flac" | "ogg" | "opus" | "aiff" | "aif" | "wma" => {
            Some("audio")
        }
        "png" | "jpg" | "jpeg" | "webp" | "gif" | "bmp" | "tif" | "tiff" | "avif" | "heic"
        | "heif" | "svg" => Some("image"),
        _ => None,
    }
}

fn normalized_extension(path: &Path) -> Option<String> {
    path.extension()
        .and_then(|extension| extension.to_str())
        .map(str::to_lowercase)
}

fn quick_file_hash(path: &Path, size: u64) -> std::io::Result<String> {
    let mut file = File::open(path)?;
    let mut material = Vec::with_capacity(QUICK_HASH_SAMPLE_BYTES * 2 + 8);
    let mut first = vec![0u8; QUICK_HASH_SAMPLE_BYTES];
    let first_count = file.read(&mut first)?;
    material.extend_from_slice(&first[..first_count]);

    if size > QUICK_HASH_SAMPLE_BYTES as u64 {
        let tail_start = size.saturating_sub(QUICK_HASH_SAMPLE_BYTES as u64);
        file.seek(SeekFrom::Start(tail_start))?;
        let mut tail = vec![0u8; QUICK_HASH_SAMPLE_BYTES];
        let tail_count = file.read(&mut tail)?;
        material.extend_from_slice(&tail[..tail_count]);
    }
    material.extend_from_slice(&size.to_le_bytes());
    Ok(format!("sha256:{}", sha256_hex(&material)))
}

fn validate_document(document: &Value) -> CommandResult<()> {
    if !document.is_object() {
        return Err(ProjectCommandError::new(
            "invalid_project_document",
            "Project document must be a JSON object",
            None,
        ));
    }
    Ok(())
}

fn serialize_stored_document(document: &StoredDocument) -> CommandResult<Vec<u8>> {
    let mut bytes = serde_json::to_vec_pretty(document).map_err(|error| {
        ProjectCommandError::new("project_serialize_failed", error.to_string(), None)
    })?;
    bytes.push(b'\n');
    Ok(bytes)
}

fn checksum_document(document: &Value) -> CommandResult<String> {
    let canonical = serde_json::to_vec(document).map_err(|error| {
        ProjectCommandError::new("project_serialize_failed", error.to_string(), None)
    })?;
    Ok(format!("sha256:{}", sha256_hex(&canonical)))
}

fn checksum_equal(left: &str, right: &str) -> bool {
    left.trim().eq_ignore_ascii_case(right.trim())
}

fn absolute_path(path: &Path) -> CommandResult<PathBuf> {
    if path.as_os_str().is_empty() {
        return Err(ProjectCommandError::new(
            "invalid_project_path",
            "Project path cannot be empty",
            None,
        ));
    }
    if path.is_absolute() {
        return Ok(path.to_path_buf());
    }
    std::env::current_dir()
        .map(|directory| directory.join(path))
        .map_err(|error| {
            ProjectCommandError::new("current_directory_failed", error.to_string(), None)
        })
}

fn atomic_write_bytes(path: &Path, bytes: &[u8]) -> CommandResult<()> {
    let parent = path.parent().ok_or_else(|| {
        ProjectCommandError::new(
            "invalid_project_path",
            "Project path has no parent directory",
            Some(path),
        )
    })?;
    fs::create_dir_all(parent)
        .map_err(|error| ProjectCommandError::io("directory_create_failed", error, parent))?;

    let mut temp_path = None;
    let mut temp_file = None;
    for attempt in 0..32u32 {
        let candidate = temporary_path(path, attempt);
        match OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&candidate)
        {
            Ok(file) => {
                temp_path = Some(candidate);
                temp_file = Some(file);
                break;
            }
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => {
                return Err(ProjectCommandError::io(
                    "temporary_file_failed",
                    error,
                    &candidate,
                ))
            }
        }
    }

    let temp_path = temp_path.ok_or_else(|| {
        ProjectCommandError::new(
            "temporary_file_failed",
            "Could not allocate a unique temporary file",
            Some(path),
        )
    })?;
    let mut file = temp_file.expect("temporary file is set together with its path");

    let write_result = (|| -> std::io::Result<()> {
        file.write_all(bytes)?;
        file.flush()?;
        file.sync_all()?;
        drop(file);
        atomic_replace(&temp_path, path)?;
        sync_parent_directory(parent)?;
        Ok(())
    })();

    if let Err(error) = write_result {
        let _ = fs::remove_file(&temp_path);
        return Err(ProjectCommandError::io("atomic_write_failed", error, path));
    }
    Ok(())
}

fn temporary_path(path: &Path, attempt: u32) -> PathBuf {
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("project");
    path.with_file_name(format!(
        ".{file_name}.{}.{}.tmp",
        std::process::id(),
        unix_time_ms().saturating_add(attempt as u64)
    ))
}

fn backup_path(path: &Path) -> PathBuf {
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("project");
    path.with_file_name(format!("{file_name}.bak"))
}

#[cfg(windows)]
fn atomic_replace(source: &Path, target: &Path) -> std::io::Result<()> {
    use std::os::windows::ffi::OsStrExt;
    type Bool = i32;
    const MOVEFILE_REPLACE_EXISTING: u32 = 0x1;
    const MOVEFILE_WRITE_THROUGH: u32 = 0x8;

    #[link(name = "kernel32")]
    extern "system" {
        fn MoveFileExW(existing: *const u16, new: *const u16, flags: u32) -> Bool;
    }

    let source_wide: Vec<u16> = source.as_os_str().encode_wide().chain(Some(0)).collect();
    let target_wide: Vec<u16> = target.as_os_str().encode_wide().chain(Some(0)).collect();
    let result = unsafe {
        MoveFileExW(
            source_wide.as_ptr(),
            target_wide.as_ptr(),
            MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
        )
    };
    if result == 0 {
        Err(std::io::Error::last_os_error())
    } else {
        Ok(())
    }
}

#[cfg(not(windows))]
fn atomic_replace(source: &Path, target: &Path) -> std::io::Result<()> {
    fs::rename(source, target)
}

#[cfg(unix)]
fn sync_parent_directory(path: &Path) -> std::io::Result<()> {
    File::open(path)?.sync_all()
}

#[cfg(not(unix))]
fn sync_parent_directory(_path: &Path) -> std::io::Result<()> {
    Ok(())
}

fn path_to_string(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

fn normalized_path_key(path: &Path) -> String {
    let value = path_to_string(path).replace('\\', "/");
    if cfg!(windows) {
        value.to_lowercase()
    } else {
        value
    }
}

fn unix_time_ms() -> u64 {
    system_time_ms(SystemTime::now()).unwrap_or_default()
}

fn system_time_ms(time: SystemTime) -> Option<u64> {
    time.duration_since(UNIX_EPOCH)
        .ok()
        .map(|duration| duration.as_millis().min(u64::MAX as u128) as u64)
}

fn default_true() -> bool {
    true
}

// Compact, dependency-free SHA-256 implementation. It is used for file integrity
// and fast media fingerprints; it is not used for authentication.
fn sha256_hex(input: &[u8]) -> String {
    const INITIAL: [u32; 8] = [
        0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab,
        0x5be0cd19,
    ];
    const K: [u32; 64] = [
        0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4,
        0xab1c5ed5, 0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe,
        0x9bdc06a7, 0xc19bf174, 0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f,
        0x4a7484aa, 0x5cb0a9dc, 0x76f988da, 0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7,
        0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967, 0x27b70a85, 0x2e1b2138, 0x4d2c6dfc,
        0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85, 0xa2bfe8a1, 0xa81a664b,
        0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070, 0x19a4c116,
        0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
        0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7,
        0xc67178f2,
    ];

    let bit_length = (input.len() as u64).wrapping_mul(8);
    let mut padded = input.to_vec();
    padded.push(0x80);
    while padded.len() % 64 != 56 {
        padded.push(0);
    }
    padded.extend_from_slice(&bit_length.to_be_bytes());

    let mut hash = INITIAL;
    for chunk in padded.chunks_exact(64) {
        let mut words = [0u32; 64];
        for (index, word) in words.iter_mut().take(16).enumerate() {
            let offset = index * 4;
            *word = u32::from_be_bytes([
                chunk[offset],
                chunk[offset + 1],
                chunk[offset + 2],
                chunk[offset + 3],
            ]);
        }
        for index in 16..64 {
            let s0 = words[index - 15].rotate_right(7)
                ^ words[index - 15].rotate_right(18)
                ^ (words[index - 15] >> 3);
            let s1 = words[index - 2].rotate_right(17)
                ^ words[index - 2].rotate_right(19)
                ^ (words[index - 2] >> 10);
            words[index] = words[index - 16]
                .wrapping_add(s0)
                .wrapping_add(words[index - 7])
                .wrapping_add(s1);
        }

        let mut a = hash[0];
        let mut b = hash[1];
        let mut c = hash[2];
        let mut d = hash[3];
        let mut e = hash[4];
        let mut f = hash[5];
        let mut g = hash[6];
        let mut h = hash[7];

        for index in 0..64 {
            let upper_e = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
            let choose = (e & f) ^ ((!e) & g);
            let temp1 = h
                .wrapping_add(upper_e)
                .wrapping_add(choose)
                .wrapping_add(K[index])
                .wrapping_add(words[index]);
            let upper_a = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
            let majority = (a & b) ^ (a & c) ^ (b & c);
            let temp2 = upper_a.wrapping_add(majority);

            h = g;
            g = f;
            f = e;
            e = d.wrapping_add(temp1);
            d = c;
            c = b;
            b = a;
            a = temp1.wrapping_add(temp2);
        }

        hash[0] = hash[0].wrapping_add(a);
        hash[1] = hash[1].wrapping_add(b);
        hash[2] = hash[2].wrapping_add(c);
        hash[3] = hash[3].wrapping_add(d);
        hash[4] = hash[4].wrapping_add(e);
        hash[5] = hash[5].wrapping_add(f);
        hash[6] = hash[6].wrapping_add(g);
        hash[7] = hash[7].wrapping_add(h);
    }

    hash.iter().map(|word| format!("{word:08x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::sync::{Arc, Barrier};

    static TEST_COUNTER: AtomicU64 = AtomicU64::new(0);

    struct TestDirectory(PathBuf);

    impl TestDirectory {
        fn new(label: &str) -> Self {
            let id = TEST_COUNTER.fetch_add(1, Ordering::Relaxed);
            let path = std::env::temp_dir()
                .join(format!("astral-lunar-{label}-{}-{id}", std::process::id()));
            fs::create_dir_all(&path).unwrap();
            Self(path)
        }
    }

    impl Drop for TestDirectory {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn document(name: &str) -> Value {
        json!({
            "schemaVersion": 1,
            "projectId": "test-project",
            "name": name,
            "timeline": { "opaque": [1, 2, 3] },
            "media": []
        })
    }

    fn request(path: &Path, document: Value) -> SaveProjectRequest {
        SaveProjectRequest {
            path: path_to_string(path),
            document,
            project_id: Some("test-project".to_string()),
            expected_checksum: None,
            create_backup: true,
        }
    }

    #[test]
    fn sha256_matches_published_vector() {
        assert_eq!(
            sha256_hex(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    #[test]
    fn save_and_load_round_trip_with_integrity() {
        let directory = TestDirectory::new("round-trip");
        let path = directory.0.join("edit.astral");
        let expected = document("Round trip");

        let save = save_project_impl(&request(&path, expected.clone())).unwrap();
        assert!(save.checksum.starts_with("sha256:"));
        assert!(save.bytes_written > 0);

        let load = load_project_impl(&path).unwrap();
        assert_eq!(load.document, expected);
        assert_eq!(load.checksum, save.checksum);
        assert!(load.verified);
        assert!(!load.recovered_from_backup);
    }

    #[test]
    fn second_save_creates_backup_and_optimistic_lock_detects_conflict() {
        let directory = TestDirectory::new("backup");
        let path = directory.0.join("edit.astral");
        let first = save_project_impl(&request(&path, document("First"))).unwrap();

        let mut second_request = request(&path, document("Second"));
        second_request.expected_checksum = Some(first.checksum.clone());
        let second = save_project_impl(&second_request).unwrap();
        assert_ne!(first.checksum, second.checksum);

        let backup = load_document_at_path(&backup_path(&path), false).unwrap();
        assert_eq!(backup.document["name"], "First");

        let mut stale_request = request(&path, document("Stale overwrite"));
        stale_request.expected_checksum = Some(first.checksum);
        let error = save_project_impl(&stale_request).unwrap_err();
        assert_eq!(error.code, "save_conflict");
    }

    #[test]
    fn corrupted_primary_recovers_read_only_from_backup() {
        let directory = TestDirectory::new("recovery");
        let path = directory.0.join("edit.astral");
        save_project_impl(&request(&path, document("Good backup"))).unwrap();
        save_project_impl(&request(&path, document("Will corrupt"))).unwrap();
        fs::write(&path, b"{ definitely not json").unwrap();

        let load = load_project_impl(&path).unwrap();
        assert!(load.recovered_from_backup);
        assert_eq!(load.document["name"], "Good backup");
    }

    #[test]
    fn newer_project_version_is_not_silently_replaced_by_an_old_backup() {
        let directory = TestDirectory::new("newer-version");
        let path = directory.0.join("edit.astral");
        save_project_impl(&request(&path, document("Old backup"))).unwrap();
        save_project_impl(&request(&path, document("New primary"))).unwrap();

        let mut stored: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        stored["formatVersion"] = json!(FILE_FORMAT_VERSION + 1);
        fs::write(&path, serde_json::to_vec_pretty(&stored).unwrap()).unwrap();

        let error = load_project_impl(&path).unwrap_err();
        assert_eq!(error.code, "project_version_unsupported");
    }

    #[test]
    fn concurrent_saves_with_the_same_revision_allow_only_one_writer() {
        let directory = TestDirectory::new("concurrent-save");
        let path = directory.0.join("edit.astral");
        let initial = save_project_impl(&request(&path, document("Initial"))).unwrap();
        let barrier = Arc::new(Barrier::new(3));

        let handles: Vec<_> = ["Writer A", "Writer B"]
            .into_iter()
            .map(|name| {
                let path = path.clone();
                let checksum = initial.checksum.clone();
                let barrier = Arc::clone(&barrier);
                std::thread::spawn(move || {
                    let mut update = request(&path, document(name));
                    update.expected_checksum = Some(checksum);
                    barrier.wait();
                    save_project_impl(&update)
                })
            })
            .collect();
        barrier.wait();
        let outcomes: Vec<_> = handles
            .into_iter()
            .map(|handle| handle.join().unwrap())
            .collect();

        assert_eq!(outcomes.iter().filter(|outcome| outcome.is_ok()).count(), 1);
        assert_eq!(
            outcomes
                .iter()
                .filter(|outcome| outcome
                    .as_ref()
                    .is_err_and(|error| error.code == "save_conflict"))
                .count(),
            1
        );
    }

    #[test]
    fn recovery_snapshot_overwrites_per_project_and_lists_corruption() {
        let directory = TestDirectory::new("autosave");
        let first_request = RecoverySnapshotRequest {
            project_id: "project-one".to_string(),
            document: document("Draft one"),
            original_path: Some("C:/projects/edit.astral".to_string()),
            base_checksum: Some("sha256:base-revision".to_string()),
            recovered_from_backup: true,
            reason: Some("debounce".to_string()),
        };
        let first = write_recovery_snapshot_impl(&directory.0, &first_request).unwrap();
        let second_request = RecoverySnapshotRequest {
            document: document("Draft two"),
            ..first_request
        };
        let second = write_recovery_snapshot_impl(&directory.0, &second_request).unwrap();
        assert_eq!(first.snapshot_id, second.snapshot_id);

        let recovered = load_recovery_snapshot_impl(&directory.0, &second.snapshot_id).unwrap();
        assert_eq!(
            recovered.original_path.as_deref(),
            Some("C:/projects/edit.astral")
        );
        assert_eq!(
            recovered.base_checksum.as_deref(),
            Some("sha256:base-revision")
        );
        assert!(recovered.recovered_from_backup);
        assert_eq!(
            recovered.recovery_snapshot_id.as_deref(),
            Some(second.snapshot_id.as_str())
        );

        fs::write(directory.0.join("broken.autosave"), b"not-json").unwrap();
        let listed = list_recovery_snapshots_impl(&directory.0).unwrap();
        assert_eq!(listed.len(), 2);
        assert!(listed.iter().any(|snapshot| snapshot.is_valid));
        assert!(listed.iter().any(|snapshot| !snapshot.is_valid));
    }

    #[test]
    fn unsaved_recovery_uses_one_stable_snapshot() {
        let directory = TestDirectory::new("unsaved-autosave");
        let first_request = RecoverySnapshotRequest {
            project_id: "random-project-one".to_string(),
            document: document("Draft one"),
            original_path: None,
            base_checksum: None,
            recovered_from_backup: false,
            reason: Some("debounce".to_string()),
        };
        let first = write_recovery_snapshot_impl(&directory.0, &first_request).unwrap();

        let second_request = RecoverySnapshotRequest {
            project_id: "random-project-two".to_string(),
            document: document("Draft two"),
            ..first_request
        };
        let second = write_recovery_snapshot_impl(&directory.0, &second_request).unwrap();

        assert_eq!(first.snapshot_id, UNSAVED_RECOVERY_SNAPSHOT_ID);
        assert_eq!(second.snapshot_id, UNSAVED_RECOVERY_SNAPSHOT_ID);
        let listed = list_recovery_snapshots_impl(&directory.0).unwrap();
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].project_id, "random-project-two");
    }

    #[test]
    fn media_inspection_supports_video_image_audio_and_deduplicates() {
        let directory = TestDirectory::new("inspect-media");
        let video = directory.0.join("clip.MP4");
        let image = directory.0.join("still.png");
        let audio = directory.0.join("voice.wav");
        fs::write(&video, b"video").unwrap();
        fs::write(&image, b"image").unwrap();
        fs::write(&audio, b"audio").unwrap();

        let results = inspect_media_impl(vec![
            path_to_string(&video),
            path_to_string(&image),
            path_to_string(&audio),
            path_to_string(&video),
        ]);
        assert_eq!(results.len(), 3);
        assert_eq!(results[0].kind.as_deref(), Some("video"));
        assert_eq!(results[1].kind.as_deref(), Some("image"));
        assert_eq!(results[2].kind.as_deref(), Some("audio"));
        assert!(results.iter().all(|item| item.quick_hash.is_some()));
    }

    #[test]
    fn relink_uses_hash_and_reports_equal_matches_as_ambiguous() {
        let directory = TestDirectory::new("relink");
        let root_one = directory.0.join("one");
        let root_two = directory.0.join("two");
        fs::create_dir_all(&root_one).unwrap();
        fs::create_dir_all(&root_two).unwrap();
        let candidate_one = root_one.join("clip.mp4");
        let candidate_two = root_two.join("clip.mp4");
        fs::write(&candidate_one, b"same-media").unwrap();
        fs::write(&candidate_two, b"same-media").unwrap();
        let hash = quick_file_hash(&candidate_one, 10).unwrap();

        let reference = MediaReference {
            asset_id: Some("asset-1".to_string()),
            path: "Z:/missing/clip.mp4".to_string(),
            size_bytes: Some(10),
            quick_hash: Some(hash),
        };
        let request = RelinkMediaRequest {
            missing: vec![reference],
            search_roots: vec![path_to_string(&directory.0)],
            max_depth: Some(4),
        };
        let result = relink_media_impl(&request).unwrap();
        assert!(result.resolved.is_empty());
        assert_eq!(result.ambiguous.len(), 1);
        assert_eq!(result.ambiguous[0].candidates.len(), 2);

        fs::write(&candidate_two, b"different!").unwrap();
        let result = relink_media_impl(&request).unwrap();
        assert_eq!(result.resolved.len(), 1);
        assert_eq!(result.resolved[0].confidence, 100);

        let renamed = root_one.join("renamed.mp4");
        fs::rename(&candidate_one, &renamed).unwrap();
        let result = relink_media_impl(&request).unwrap();
        assert_eq!(result.resolved.len(), 1);
        assert_eq!(
            result.resolved[0].replacement_path,
            path_to_string(&renamed)
        );
        assert_eq!(result.resolved[0].reason, "quick-hash-renamed");
    }
}
