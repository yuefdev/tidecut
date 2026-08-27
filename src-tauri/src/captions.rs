use serde::Serialize;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

const MAX_CAPTION_BYTES: u64 = 32 * 1024 * 1024;
static CAPTION_WRITE_LOCK: Mutex<()> = Mutex::new(());

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CaptionFileError {
    code: String,
    message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    path: Option<String>,
}

impl CaptionFileError {
    fn new(code: &str, message: impl Into<String>, path: Option<&Path>) -> Self {
        Self {
            code: code.to_string(),
            message: message.into(),
            path: path.map(|value| value.to_string_lossy().into_owned()),
        }
    }

    fn io(code: &str, error: std::io::Error, path: &Path) -> Self {
        Self::new(code, error.to_string(), Some(path))
    }
}

type CommandResult<T> = Result<T, CaptionFileError>;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CaptionFilePayload {
    path: String,
    file_name: String,
    extension: String,
    content: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CaptionWriteResult {
    path: String,
    bytes_written: u64,
}

#[tauri::command]
pub fn read_caption_file(path: String) -> CommandResult<CaptionFilePayload> {
    let path = PathBuf::from(path);
    validate_caption_extension(&path)?;
    let metadata = fs::metadata(&path)
        .map_err(|error| CaptionFileError::io("caption_read_failed", error, &path))?;
    if !metadata.is_file() {
        return Err(CaptionFileError::new(
            "caption_not_a_file",
            "Seçilen altyazı yolu bir dosya değil.",
            Some(&path),
        ));
    }
    if metadata.len() > MAX_CAPTION_BYTES {
        return Err(CaptionFileError::new(
            "caption_too_large",
            "Altyazı dosyası 32 MB sınırını aşıyor.",
            Some(&path),
        ));
    }

    let bytes = fs::read(&path)
        .map_err(|error| CaptionFileError::io("caption_read_failed", error, &path))?;
    let bytes = bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]).unwrap_or(&bytes);
    let content = String::from_utf8(bytes.to_vec()).map_err(|_| {
        CaptionFileError::new(
            "caption_invalid_encoding",
            "Altyazı dosyası geçerli UTF-8 değil.",
            Some(&path),
        )
    })?;
    let file_name = path
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("captions")
        .to_string();
    let extension = path
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();

    Ok(CaptionFilePayload {
        path: path.to_string_lossy().into_owned(),
        file_name,
        extension,
        content,
    })
}

#[tauri::command]
pub fn write_caption_file(path: String, content: String) -> CommandResult<CaptionWriteResult> {
    let _write_guard = CAPTION_WRITE_LOCK.lock().map_err(|_| {
        CaptionFileError::new(
            "caption_write_lock_failed",
            "Altyazı yazma kilidi kullanılamıyor.",
            None,
        )
    })?;
    let path = PathBuf::from(path);
    validate_caption_extension(&path)?;
    if content.len() as u64 > MAX_CAPTION_BYTES {
        return Err(CaptionFileError::new(
            "caption_too_large",
            "Altyazı çıktısı 32 MB sınırını aşıyor.",
            Some(&path),
        ));
    }
    let parent = path.parent().ok_or_else(|| {
        CaptionFileError::new(
            "caption_invalid_path",
            "Altyazı dosyası için geçerli bir klasör seçilmedi.",
            Some(&path),
        )
    })?;
    fs::create_dir_all(parent)
        .map_err(|error| CaptionFileError::io("caption_write_failed", error, parent))?;

    let temporary = temporary_path(&path, "tmp");
    let backup = temporary_path(&path, "previous");
    let write_result = (|| -> std::io::Result<()> {
        let mut file = OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&temporary)?;
        file.write_all(content.as_bytes())?;
        file.sync_all()?;
        if path.exists() {
            if backup.exists() {
                fs::remove_file(&backup)?;
            }
            fs::rename(&path, &backup)?;
        }
        if let Err(error) = fs::rename(&temporary, &path) {
            if backup.exists() {
                let _ = fs::rename(&backup, &path);
            }
            return Err(error);
        }
        if backup.exists() {
            fs::remove_file(&backup)?;
        }
        Ok(())
    })();
    if let Err(error) = write_result {
        let _ = fs::remove_file(&temporary);
        return Err(CaptionFileError::io("caption_write_failed", error, &path));
    }

    Ok(CaptionWriteResult {
        path: path.to_string_lossy().into_owned(),
        bytes_written: content.len() as u64,
    })
}

fn validate_caption_extension(path: &Path) -> CommandResult<()> {
    let extension = path
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    if matches!(extension.as_str(), "srt" | "vtt" | "ass" | "ssa") {
        return Ok(());
    }
    Err(CaptionFileError::new(
        "caption_format_unsupported",
        "Yalnızca SRT, VTT ve ASS altyazı dosyaları destekleniyor.",
        Some(path),
    ))
}

fn temporary_path(path: &Path, suffix: &str) -> PathBuf {
    let mut candidate = path.as_os_str().to_os_string();
    candidate.push(format!(".{}.{}", std::process::id(), suffix));
    PathBuf::from(candidate)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_supported_extensions_case_insensitively() {
        assert!(validate_caption_extension(Path::new("sample.SRT")).is_ok());
        assert!(validate_caption_extension(Path::new("sample.vtt")).is_ok());
        assert!(validate_caption_extension(Path::new("sample.ass")).is_ok());
        assert!(validate_caption_extension(Path::new("sample.txt")).is_err());
    }

    #[test]
    fn writes_reads_and_safely_replaces_caption_text() {
        let root = std::env::temp_dir().join(format!(
            "astral-caption-test-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&root).unwrap();
        let path = root.join("Türkçe altyazı.srt");

        let first = write_caption_file(path.to_string_lossy().into_owned(), "ilk".into()).unwrap();
        assert_eq!(first.bytes_written, 3);
        assert_eq!(
            read_caption_file(path.to_string_lossy().into_owned())
                .unwrap()
                .content,
            "ilk"
        );

        write_caption_file(path.to_string_lossy().into_owned(), "ikinci".into()).unwrap();
        let loaded = read_caption_file(path.to_string_lossy().into_owned()).unwrap();
        assert_eq!(loaded.content, "ikinci");
        assert_eq!(loaded.extension, "srt");

        fs::remove_dir_all(root).unwrap();
    }
}
