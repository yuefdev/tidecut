use std::{
    collections::HashSet,
    io::{self, Read},
    process::{Child, Output},
    sync::{Mutex, OnceLock},
    thread,
    time::Duration,
};

static CANCELLED_OPERATIONS: OnceLock<Mutex<HashSet<String>>> = OnceLock::new();

fn cancelled_operations() -> &'static Mutex<HashSet<String>> {
    CANCELLED_OPERATIONS.get_or_init(|| Mutex::new(HashSet::new()))
}

fn valid_operation_id(operation_id: &str) -> bool {
    !operation_id.is_empty()
        && operation_id.len() <= 128
        && operation_id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
}

#[tauri::command]
pub fn begin_smart_shorts_analysis(operation_id: String) -> Result<(), String> {
    if !valid_operation_id(&operation_id) {
        return Err("Geçersiz Akıllı Shorts işlem kimliği.".to_owned());
    }
    cancelled_operations()
        .lock()
        .map_err(|_| "Akıllı Shorts iptal durumu kullanılamıyor.".to_owned())?
        .remove(&operation_id);
    Ok(())
}

#[tauri::command]
pub fn cancel_smart_shorts_analysis(operation_id: String) -> Result<(), String> {
    if !valid_operation_id(&operation_id) {
        return Err("Geçersiz Akıllı Shorts işlem kimliği.".to_owned());
    }
    cancelled_operations()
        .lock()
        .map_err(|_| "Akıllı Shorts iptal durumu kullanılamıyor.".to_owned())?
        .insert(operation_id);
    Ok(())
}

#[tauri::command]
pub fn finish_smart_shorts_analysis(operation_id: String) -> Result<(), String> {
    if !valid_operation_id(&operation_id) {
        return Err("Geçersiz Akıllı Shorts işlem kimliği.".to_owned());
    }
    cancelled_operations()
        .lock()
        .map_err(|_| "Akıllı Shorts iptal durumu kullanılamıyor.".to_owned())?
        .remove(&operation_id);
    Ok(())
}

pub(crate) fn is_cancelled(operation_id: Option<&str>) -> bool {
    let Some(operation_id) = operation_id else {
        return false;
    };
    cancelled_operations()
        .lock()
        .map(|operations| operations.contains(operation_id))
        .unwrap_or(true)
}

pub(crate) enum CommandWaitError {
    Cancelled,
    Io(io::Error),
}

pub(crate) fn wait_for_output(
    mut child: Child,
    operation_id: Option<&str>,
) -> Result<Output, CommandWaitError> {
    // Drain stdout/stderr on dedicated threads. A child that fills its OS pipe
    // buffer (e.g. ffmpeg ebur128 at -loglevel info over a long clip) blocks on
    // write and never exits, so a wait loop that only reads after exit would
    // deadlock. Reading concurrently keeps the pipes clear while we poll.
    let stdout_reader = child.stdout.take().map(spawn_pipe_reader);
    let stderr_reader = child.stderr.take().map(spawn_pipe_reader);

    let result = loop {
        if is_cancelled(operation_id) {
            let _ = child.kill();
            let _ = child.wait();
            break Err(CommandWaitError::Cancelled);
        }
        match child.try_wait() {
            Ok(Some(status)) => break Ok(status),
            Ok(None) => thread::sleep(Duration::from_millis(40)),
            Err(error) => break Err(CommandWaitError::Io(error)),
        }
    };

    // Join the reader threads exactly once, regardless of how the loop ended.
    let stdout = stdout_reader
        .map(|handle| handle.join().unwrap_or_default())
        .unwrap_or_default();
    let stderr = stderr_reader
        .map(|handle| handle.join().unwrap_or_default())
        .unwrap_or_default();

    let status = result?;
    Ok(Output {
        status,
        stdout,
        stderr,
    })
}

fn spawn_pipe_reader<R: Read + Send + 'static>(mut pipe: R) -> thread::JoinHandle<Vec<u8>> {
    thread::spawn(move || {
        let mut buffer = Vec::new();
        let _ = pipe.read_to_end(&mut buffer);
        buffer
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cancellation_is_scoped_and_can_be_finished() {
        let id = "smart-shorts-test-operation".to_owned();
        begin_smart_shorts_analysis(id.clone()).unwrap();
        assert!(!is_cancelled(Some(&id)));
        cancel_smart_shorts_analysis(id.clone()).unwrap();
        assert!(is_cancelled(Some(&id)));
        finish_smart_shorts_analysis(id.clone()).unwrap();
        assert!(!is_cancelled(Some(&id)));
    }

    #[test]
    fn invalid_operation_ids_fail_closed() {
        assert!(begin_smart_shorts_analysis("".to_owned()).is_err());
        assert!(cancel_smart_shorts_analysis("bad id".to_owned()).is_err());
    }
}
