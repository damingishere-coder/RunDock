// @group APIEndpoints : Script file management — save, list, read, delete, and run scripts

use crate::api::error::ApiError;
use crate::config::paths::scripts_dir;
use crate::daemon::state::DaemonState;
use crate::process::instance::{LogLine, LogStream};
use crate::process::tree::ProcessTreeGuard;
use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::sse::Event,
    response::Sse,
    routing::get,
    Json, Router,
};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::convert::Infallible;
use std::path::{Path as FsPath, PathBuf};
use std::process::Stdio;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use tokio::io::{AsyncRead, AsyncReadExt, BufReader};
use tokio::process::Command;
use tokio::sync::broadcast;

pub fn router(state: Arc<DaemonState>) -> Router {
    Router::new()
        .route("/", get(list_scripts).post(save_script))
        .route("/{name}", get(get_script).delete(delete_script))
        .route("/{name}/run", get(run_script))
        .with_state(state)
}

// @group Types : Script save request body
#[derive(Deserialize)]
struct SaveScriptRequest {
    name: String,
    language: String,
    content: String,
}

// @group Types : Script metadata returned by list/save
#[derive(Serialize)]
struct ScriptMeta {
    name: String,
    path: String,
    language: String,
    size_bytes: u64,
    modified_at: String,
}

#[derive(Clone, Copy)]
struct LanguageSpec {
    language: &'static str,
    extension: &'static str,
    program: &'static str,
    prefix_args: &'static [&'static str],
}

const LANGUAGE_SPECS: &[LanguageSpec] = &[
    LanguageSpec {
        language: "python",
        extension: "py",
        program: "python",
        prefix_args: &[],
    },
    LanguageSpec {
        language: "node",
        extension: "js",
        program: "node",
        prefix_args: &[],
    },
    LanguageSpec {
        language: "ts-node",
        extension: "ts",
        program: "ts-node",
        prefix_args: &[],
    },
    LanguageSpec {
        language: "powershell",
        extension: "ps1",
        program: "powershell",
        prefix_args: &[],
    },
    LanguageSpec {
        language: "bash",
        extension: "sh",
        program: "bash",
        prefix_args: &[],
    },
    LanguageSpec {
        language: "cmd",
        extension: "bat",
        program: "cmd",
        prefix_args: &[],
    },
    LanguageSpec {
        language: "ruby",
        extension: "rb",
        program: "ruby",
        prefix_args: &[],
    },
    LanguageSpec {
        language: "php",
        extension: "php",
        program: "php",
        prefix_args: &[],
    },
    LanguageSpec {
        language: "perl",
        extension: "pl",
        program: "perl",
        prefix_args: &[],
    },
    LanguageSpec {
        language: "lua",
        extension: "lua",
        program: "lua",
        prefix_args: &[],
    },
    LanguageSpec {
        language: "groovy",
        extension: "groovy",
        program: "groovy",
        prefix_args: &[],
    },
    LanguageSpec {
        language: "kotlin",
        extension: "kts",
        program: "kotlinc",
        prefix_args: &["-script"],
    },
    LanguageSpec {
        language: "scala",
        extension: "sc",
        program: "scala",
        prefix_args: &[],
    },
    LanguageSpec {
        language: "clj",
        extension: "clj",
        program: "clj",
        prefix_args: &[],
    },
    LanguageSpec {
        language: "dotnet-script",
        extension: "csx",
        program: "dotnet-script",
        prefix_args: &[],
    },
    LanguageSpec {
        language: "dotnet-fsi",
        extension: "fsx",
        program: "dotnet",
        prefix_args: &["fsi"],
    },
    LanguageSpec {
        language: "go",
        extension: "go",
        program: "go",
        prefix_args: &["run"],
    },
    LanguageSpec {
        language: "Rscript",
        extension: "r",
        program: "Rscript",
        prefix_args: &[],
    },
    LanguageSpec {
        language: "julia",
        extension: "jl",
        program: "julia",
        prefix_args: &[],
    },
    LanguageSpec {
        language: "swift",
        extension: "swift",
        program: "swift",
        prefix_args: &[],
    },
    LanguageSpec {
        language: "elixir",
        extension: "exs",
        program: "elixir",
        prefix_args: &[],
    },
    LanguageSpec {
        language: "escript",
        extension: "erl",
        program: "escript",
        prefix_args: &[],
    },
    LanguageSpec {
        language: "runghc",
        extension: "hs",
        program: "runghc",
        prefix_args: &[],
    },
    LanguageSpec {
        language: "ocaml",
        extension: "ml",
        program: "ocaml",
        prefix_args: &[],
    },
    LanguageSpec {
        language: "tclsh",
        extension: "tcl",
        program: "tclsh",
        prefix_args: &[],
    },
    LanguageSpec {
        language: "awk",
        extension: "awk",
        program: "awk",
        prefix_args: &["-f"],
    },
];

fn language_for_name(language: &str) -> Option<LanguageSpec> {
    LANGUAGE_SPECS
        .iter()
        .copied()
        .find(|spec| spec.language == language)
}

fn language_for_extension(extension: &str) -> Option<LanguageSpec> {
    LANGUAGE_SPECS
        .iter()
        .copied()
        .find(|spec| spec.extension == extension)
}

fn validate_script_name(name: &str) -> Result<(), ApiError> {
    if name.is_empty()
        || name.len() > 128
        || !name
            .chars()
            .all(|character| character.is_alphanumeric() || character == '-' || character == '_')
    {
        return Err(ApiError::bad_request(
            "script name must contain 1 to 128 letters, numbers, '-' or '_' characters",
        ));
    }
    Ok(())
}

fn find_script_path(dir: &FsPath, name: &str) -> Result<PathBuf, ApiError> {
    validate_script_name(name)?;
    let canonical_root =
        std::fs::canonicalize(dir).map_err(|_| ApiError::not_found("scripts dir not found"))?;
    let mut matches = Vec::new();
    for entry in std::fs::read_dir(dir)
        .map_err(|error| ApiError::internal(format!("failed to list scripts: {error}")))?
    {
        let entry = entry
            .map_err(|error| ApiError::internal(format!("failed to inspect script: {error}")))?;
        let path = entry.path();
        if path.file_stem().and_then(|stem| stem.to_str()) != Some(name) {
            continue;
        }
        let metadata = std::fs::symlink_metadata(&path)
            .map_err(|error| ApiError::internal(format!("failed to inspect script: {error}")))?;
        if metadata.file_type().is_symlink() {
            return Err(ApiError::bad_request(
                "symbolic-link scripts are not allowed",
            ));
        }
        if metadata.is_file() {
            let canonical_path = std::fs::canonicalize(&path).map_err(|error| {
                ApiError::internal(format!("failed to resolve script path: {error}"))
            })?;
            if !canonical_path.starts_with(&canonical_root) {
                return Err(ApiError::bad_request(
                    "script path escapes the scripts directory",
                ));
            }
            matches.push(canonical_path);
        }
    }
    match matches.len() {
        0 => Err(ApiError::not_found(format!("script '{name}' not found"))),
        1 => Ok(matches.remove(0)),
        _ => Err(ApiError::conflict(format!(
            "script '{name}' has multiple extensions; save it again to repair the conflict"
        ))),
    }
}

struct ScriptSnapshot {
    path: PathBuf,
}

fn open_script_nofollow(path: &FsPath) -> Result<std::fs::File, ApiError> {
    let mut options = std::fs::OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NOFOLLOW);
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        options.custom_flags(0x0020_0000); // FILE_FLAG_OPEN_REPARSE_POINT
    }
    let file = options
        .open(path)
        .map_err(|error| ApiError::bad_request(format!("failed to safely open script: {error}")))?;
    let metadata = file
        .metadata()
        .map_err(|error| ApiError::internal(format!("failed to inspect opened script: {error}")))?;
    if !metadata.is_file() {
        return Err(ApiError::bad_request("script is not a regular file"));
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x0000_0400;
        if metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
            return Err(ApiError::bad_request(
                "script reparse points are not allowed",
            ));
        }
    }
    Ok(file)
}

impl Drop for ScriptSnapshot {
    fn drop(&mut self) {
        if let Err(error) = std::fs::remove_file(&self.path) {
            if error.kind() != std::io::ErrorKind::NotFound {
                tracing::warn!(path = %self.path.display(), %error, "script snapshot cleanup failed");
            }
        }
    }
}

fn create_script_snapshot(
    source: &FsPath,
    scripts_root: &FsPath,
    extension: &str,
) -> Result<ScriptSnapshot, ApiError> {
    use std::io::{Read, Write};

    const MAX_SCRIPT_BYTES: u64 = 1024 * 1024;
    let source_file = open_script_nofollow(source)?;
    let metadata = source_file
        .metadata()
        .map_err(|error| ApiError::internal(format!("failed to inspect script: {error}")))?;
    if metadata.len() > MAX_SCRIPT_BYTES {
        return Err(ApiError::bad_request("script content exceeds 1 MiB"));
    }
    let mut content = Vec::new();
    source_file
        .take(MAX_SCRIPT_BYTES + 1)
        .read_to_end(&mut content)
        .map_err(|error| ApiError::internal(format!("failed to snapshot script: {error}")))?;
    if content.len() as u64 > MAX_SCRIPT_BYTES {
        return Err(ApiError::bad_request("script content exceeds 1 MiB"));
    }

    let snapshot_path =
        scripts_root.join(format!(".alter-run-{}.{}", uuid::Uuid::new_v4(), extension));
    let mut snapshot_file = std::fs::OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(&snapshot_path)
        .map_err(|error| {
            ApiError::internal(format!("failed to create script snapshot: {error}"))
        })?;
    if let Err(error) = snapshot_file
        .write_all(&content)
        .and_then(|_| snapshot_file.sync_all())
    {
        let _ = std::fs::remove_file(&snapshot_path);
        return Err(ApiError::internal(format!(
            "failed to persist script snapshot: {error}"
        )));
    }
    Ok(ScriptSnapshot {
        path: snapshot_path,
    })
}

// @group APIEndpoints > Scripts : POST /scripts — save a script to disk
async fn save_script(
    State(state): State<Arc<DaemonState>>,
    Json(req): Json<SaveScriptRequest>,
) -> Result<(StatusCode, Json<Value>), ApiError> {
    validate_script_name(&req.name)?;
    if req.content.len() > 1024 * 1024 {
        return Err(ApiError::bad_request("script content exceeds 1 MiB"));
    }
    let _script_guard = state.script_mutation_lock.lock().await;
    let dir = scripts_dir();
    std::fs::create_dir_all(&dir)
        .map_err(|e| ApiError::internal(format!("failed to create scripts dir: {e}")))?;

    let language = language_for_name(&req.language)
        .ok_or_else(|| ApiError::bad_request("unsupported script language"))?;
    let filename = format!("{}.{}", req.name, language.extension);
    let path = dir.join(&filename);
    if std::fs::symlink_metadata(&path).is_ok_and(|metadata| metadata.file_type().is_symlink()) {
        return Err(ApiError::bad_request(
            "symbolic-link scripts are not allowed",
        ));
    }

    let mut previous_paths = Vec::new();
    for entry in std::fs::read_dir(&dir)
        .map_err(|error| ApiError::internal(format!("failed to list scripts: {error}")))?
    {
        let entry = entry
            .map_err(|error| ApiError::internal(format!("failed to inspect script: {error}")))?;
        let previous_path = entry.path();
        if previous_path.file_stem().and_then(|stem| stem.to_str()) != Some(req.name.as_str())
            || previous_path == path
        {
            continue;
        }
        let metadata = std::fs::symlink_metadata(&previous_path).map_err(|error| {
            ApiError::internal(format!("failed to inspect previous script: {error}"))
        })?;
        if metadata.file_type().is_symlink() {
            return Err(ApiError::bad_request(
                "refusing to replace a symbolic-link script",
            ));
        }
        if metadata.is_file() {
            previous_paths.push(previous_path);
        }
    }

    if previous_paths.len() > 1 || (!previous_paths.is_empty() && path.exists()) {
        return Err(ApiError::conflict(format!(
            "script '{}' has multiple extensions; remove the conflicting files before changing its language",
            req.name
        )));
    }

    crate::config::atomic_file::write_with_backup(&path, req.content.as_bytes(), None)
        .map_err(|e| ApiError::internal(format!("failed to write script: {e}")))?;

    // Move the old extension aside before deleting it so a cleanup failure can be rolled back
    // without ever exposing two public script names or losing the prior content.
    if let Some(previous_path) = previous_paths.into_iter().next() {
        let staging_path = dir.join(format!(".alter-run-migrate-{}", uuid::Uuid::new_v4()));
        if let Err(error) = std::fs::rename(&previous_path, &staging_path) {
            let cleanup_error = std::fs::remove_file(&path).err();
            return Err(ApiError::internal(format!(
                "failed to stage the previous script extension: {error}; replacement cleanup: {}",
                cleanup_error
                    .map(|failure| failure.to_string())
                    .unwrap_or_else(|| "ok".to_string())
            )));
        }
        if let Err(error) = std::fs::remove_file(&staging_path) {
            let restore_error = std::fs::rename(&staging_path, &previous_path).err();
            let cleanup_error = std::fs::remove_file(&path).err();
            return Err(ApiError::internal(format!(
                "failed to finalize the script extension change: {error}; previous script restore: {}; replacement cleanup: {}",
                restore_error
                    .map(|failure| failure.to_string())
                    .unwrap_or_else(|| "ok".to_string()),
                cleanup_error
                    .map(|failure| failure.to_string())
                    .unwrap_or_else(|| "ok".to_string())
            )));
        }
    }

    let path_str = path.to_string_lossy().to_string();
    Ok((
        StatusCode::CREATED,
        Json(json!({
            "name": req.name,
            "filename": filename,
            "path": path_str,
            "language": req.language,
        })),
    ))
}

// @group APIEndpoints > Scripts : GET /scripts — list all saved scripts
async fn list_scripts(State(state): State<Arc<DaemonState>>) -> Result<Json<Value>, ApiError> {
    let _script_guard = state.script_mutation_lock.lock().await;
    let dir = scripts_dir();
    let mut scripts: Vec<ScriptMeta> = Vec::new();

    match std::fs::read_dir(&dir) {
        Ok(entries) => {
            for entry in entries {
                let entry = entry.map_err(|error| {
                    ApiError::internal(format!("failed to inspect script: {error}"))
                })?;
                let path = entry.path();
                if path
                    .file_name()
                    .and_then(|name| name.to_str())
                    .is_some_and(|name| name.starts_with(".alter-run-"))
                {
                    continue;
                }
                let metadata = std::fs::symlink_metadata(&path).map_err(|error| {
                    ApiError::internal(format!("failed to inspect script: {error}"))
                })?;
                if metadata.file_type().is_symlink() || !metadata.is_file() {
                    continue;
                }
                let name = path
                    .file_stem()
                    .and_then(|n| n.to_str())
                    .unwrap_or("")
                    .to_string();
                let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
                let size_bytes = metadata.len();
                let modified_at = metadata
                    .modified()
                    .ok()
                    .and_then(|t| {
                        let secs = t.duration_since(std::time::UNIX_EPOCH).ok()?.as_secs();
                        let dt = chrono::DateTime::<Utc>::from_timestamp(secs as i64, 0)?;
                        Some(dt.to_rfc3339())
                    })
                    .unwrap_or_default();

                // Guess language from extension (reverse map)
                let language = language_for_extension(ext)
                    .map(|spec| spec.language)
                    .unwrap_or("text")
                    .to_string();

                scripts.push(ScriptMeta {
                    name,
                    path: path.to_string_lossy().to_string(),
                    language,
                    size_bytes,
                    modified_at,
                });
            }
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => {
            return Err(ApiError::internal(format!(
                "failed to list scripts: {error}"
            )))
        }
    }

    scripts.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(Json(json!({ "scripts": scripts })))
}

// @group APIEndpoints > Scripts : GET /scripts/{name} — read script content
async fn get_script(
    State(state): State<Arc<DaemonState>>,
    Path(name): Path<String>,
) -> Result<Json<Value>, ApiError> {
    let _script_guard = state.script_mutation_lock.lock().await;
    let dir = scripts_dir();
    let path = find_script_path(&dir, &name)?;
    let file = open_script_nofollow(&path)?;
    let metadata = file
        .metadata()
        .map_err(|error| ApiError::internal(format!("failed to inspect script: {error}")))?;
    if metadata.len() > 1024 * 1024 {
        return Err(ApiError::bad_request("script content exceeds 1 MiB"));
    }
    use std::io::Read;
    let mut content = String::new();
    file.take(1024 * 1024 + 1)
        .read_to_string(&mut content)
        .map_err(|e| ApiError::internal(format!("failed to read script: {e}")))?;
    if content.len() > 1024 * 1024 {
        return Err(ApiError::bad_request("script content exceeds 1 MiB"));
    }
    let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
    let language = language_for_extension(ext);

    Ok(Json(json!({
        "name": name,
        "path": path.to_string_lossy(),
        "content": content,
        "language": language.map(|spec| spec.language).unwrap_or("text"),
        "interpreter": language.map(|spec| spec.program),
        "prefix_args": language.map(|spec| spec.prefix_args).unwrap_or_default(),
    })))
}

// @group APIEndpoints > Scripts : DELETE /scripts/{name} — remove script file
async fn delete_script(
    State(state): State<Arc<DaemonState>>,
    Path(name): Path<String>,
) -> Result<Json<Value>, ApiError> {
    let _script_guard = state.script_mutation_lock.lock().await;
    let path = find_script_path(&scripts_dir(), &name)?;
    std::fs::remove_file(path)
        .map_err(|e| ApiError::internal(format!("failed to delete script: {e}")))?;

    Ok(Json(json!({ "success": true })))
}

// @group APIEndpoints > Scripts : GET /scripts/{name}/run — spawn script and stream output via SSE
async fn run_script(
    State(state): State<Arc<DaemonState>>,
    Path(name): Path<String>,
) -> Result<Sse<impl futures::Stream<Item = Result<Event, Infallible>>>, ApiError> {
    let dir = scripts_dir();
    let (snapshot, ext, language) = {
        let _script_guard = state.script_mutation_lock.lock().await;
        let script_path = find_script_path(&dir, &name)?;
        let ext = script_path
            .extension()
            .and_then(|extension| extension.to_str())
            .unwrap_or("")
            .to_string();
        let language = language_for_extension(&ext)
            .ok_or_else(|| ApiError::bad_request("script file type is not executable"))?;
        let snapshot = create_script_snapshot(&script_path, &dir, &ext)?;
        (snapshot, ext, language)
    };
    let script_str = snapshot.path.to_string_lossy().to_string();

    // @group BusinessLogic > Run : Spawn the script process directly (not via process manager)
    #[cfg(target_os = "windows")]
    let mut cmd = {
        // .bat/.cmd files must be run via "cmd /C <script>" — not "cmd /C cmd <script>"
        let is_batch = ext == "bat" || ext == "cmd";
        if is_batch {
            let mut c = Command::new("cmd");
            c.args(["/C", &script_str]);
            c
        } else {
            let mut c = Command::new(language.program);
            c.args(language.prefix_args);
            c.arg(&script_str);
            c
        }
    };
    #[cfg(not(target_os = "windows"))]
    let mut cmd = {
        let mut c = Command::new(language.program);
        c.args(language.prefix_args);
        c.arg(&script_str);
        c
    };

    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        cmd.as_std_mut().process_group(0);
    }
    #[cfg(windows)]
    {
        cmd.creation_flags(0x0800_0000 | 0x0100_0000); // CREATE_NO_WINDOW | CREATE_BREAKAWAY_FROM_JOB
    }

    cmd.stdout(Stdio::piped());
    cmd.stderr(Stdio::piped());
    cmd.kill_on_drop(false);

    // Set working directory to scripts dir
    cmd.current_dir(&dir);

    let permit = Arc::clone(&state.script_run_limit)
        .try_acquire_owned()
        .map_err(|_| ApiError {
            status: StatusCode::TOO_MANY_REQUESTS,
            message: "Too many scripts are already running".into(),
        })?;
    let mut child = cmd
        .spawn()
        .map_err(|e| ApiError::internal(format!("failed to spawn script: {e}")))?;
    let child_pid = child
        .id()
        .ok_or_else(|| ApiError::internal("spawned script has no process id"))?;
    let process_tree = match ProcessTreeGuard::new(child_pid, &format!("script-{name}")) {
        Ok(process_tree) => process_tree,
        Err(error) => {
            let _ = child.kill().await;
            let _ = child.wait().await;
            return Err(ApiError::internal(format!(
                "failed to contain script process tree: {error}"
            )));
        }
    };

    let stdout = child.stdout.take().expect("stdout piped");
    let stderr = child.stderr.take().expect("stderr piped");

    // @group BusinessLogic > Run : Broadcast channel for merging stdout + stderr
    let (tx, _) = broadcast::channel::<LogLine>(512);
    let tx_out = tx.clone();
    let tx_err = tx.clone();
    let mut rx = tx.subscribe();
    let total_bytes = Arc::new(AtomicUsize::new(0));
    let total_stdout = Arc::clone(&total_bytes);

    let dummy_id = uuid::Uuid::new_v4();

    // Stream stdout
    tokio::spawn(async move {
        forward_script_output(
            BufReader::new(stdout),
            LogStream::Stdout,
            tx_out,
            dummy_id,
            total_stdout,
        )
        .await;
    });

    // Stream stderr
    tokio::spawn(async move {
        forward_script_output(
            BufReader::new(stderr),
            LogStream::Stderr,
            tx_err,
            dummy_id,
            total_bytes,
        )
        .await;
    });

    drop(tx);

    // @group BusinessLogic > Run : SSE event stream — yields log lines then a done event
    let event_stream = async_stream::stream! {
        const MAX_SCRIPT_OUTPUT_LINES: usize = 10_000;
        const MAX_SCRIPT_RUNTIME: tokio::time::Duration = tokio::time::Duration::from_secs(15 * 60);
        let mut exit_code: Option<i32> = None;
        let mut output_lines = 0usize;
        let deadline = tokio::time::sleep(MAX_SCRIPT_RUNTIME);
        tokio::pin!(deadline);
        let _permit = permit;
        let _snapshot = snapshot;
        let _process_tree = process_tree;
        loop {
            tokio::select! {
                status = child.wait() => {
                    exit_code = status.ok().and_then(|status| status.code());
                    // Drain remaining messages briefly
                    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
                    while let Ok(line) = rx.try_recv() {
                        let data = serde_json::json!({
                            "stream": if line.stream == LogStream::Stderr { "stderr" } else { "stdout" },
                            "content": line.content,
                        });
                        yield Ok(Event::default().data(data.to_string()));
                    }
                    break;
                }
                // New log line
                msg = rx.recv() => {
                    match msg {
                        Ok(line) => {
                            if line.content == "\0__output_limit__\0" {
                                let _ = child.kill().await;
                                exit_code = child.wait().await.ok().and_then(|status| status.code());
                                yield Ok(Event::default().data(serde_json::json!({
                                    "error": "script stopped after exceeding the output byte or line limit"
                                }).to_string()));
                                break;
                            }
                            output_lines += 1;
                            if output_lines > MAX_SCRIPT_OUTPUT_LINES {
                                let _ = child.kill().await;
                                exit_code = child.wait().await.ok().and_then(|status| status.code());
                                yield Ok(Event::default().data(serde_json::json!({
                                    "error": "script stopped after exceeding 10000 output lines"
                                }).to_string()));
                                break;
                            }
                            let data = serde_json::json!({
                                "stream": if line.stream == LogStream::Stderr { "stderr" } else { "stdout" },
                                "content": line.content,
                            });
                            yield Ok(Event::default().data(data.to_string()));
                        }
                        Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                        Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {
                            let _ = child.kill().await;
                            exit_code = child.wait().await.ok().and_then(|status| status.code());
                            yield Ok(Event::default().data(serde_json::json!({
                                "error": "script stopped because output exceeded the streaming buffer"
                            }).to_string()));
                            break;
                        }
                    }
                }
                _ = &mut deadline => {
                    let _ = child.kill().await;
                    exit_code = child.wait().await.ok().and_then(|status| status.code());
                    yield Ok(Event::default().data(serde_json::json!({
                        "error": "script stopped after the 15 minute runtime limit"
                    }).to_string()));
                    break;
                }
            }
        }
        // Send final done event with exit code
        let done_data = serde_json::json!({ "done": true, "exit_code": exit_code });
        yield Ok(Event::default().data(done_data.to_string()));
    };

    Ok(Sse::new(event_stream))
}

async fn forward_script_output<R: AsyncRead + Unpin>(
    mut reader: BufReader<R>,
    stream: LogStream,
    tx: broadcast::Sender<LogLine>,
    process_id: uuid::Uuid,
    total_bytes: Arc<AtomicUsize>,
) {
    const MAX_SCRIPT_OUTPUT_BYTES: usize = 5 * 1024 * 1024;
    const MAX_SCRIPT_LINE_BYTES: usize = 64 * 1024;
    let mut chunk = [0u8; 8192];
    let mut line = Vec::new();
    loop {
        let read = match reader.read(&mut chunk).await {
            Ok(0) => break,
            Ok(read) => read,
            Err(_) => return,
        };
        let previous = total_bytes.fetch_add(read, Ordering::Relaxed);
        if previous.saturating_add(read) > MAX_SCRIPT_OUTPUT_BYTES {
            let _ = tx.send(LogLine {
                timestamp: Utc::now(),
                process_id,
                stream: stream.clone(),
                content: "\0__output_limit__\0".to_string(),
            });
            return;
        }
        for byte in &chunk[..read] {
            if *byte == b'\n' {
                if line.last() == Some(&b'\r') {
                    line.pop();
                }
                let content = String::from_utf8_lossy(&line).into_owned();
                line.clear();
                let _ = tx.send(LogLine {
                    timestamp: Utc::now(),
                    process_id,
                    stream: stream.clone(),
                    content,
                });
            } else {
                line.push(*byte);
                if line.len() > MAX_SCRIPT_LINE_BYTES {
                    let _ = tx.send(LogLine {
                        timestamp: Utc::now(),
                        process_id,
                        stream: stream.clone(),
                        content: "\0__output_limit__\0".to_string(),
                    });
                    return;
                }
            }
        }
    }
    if !line.is_empty() {
        let _ = tx.send(LogLine {
            timestamp: Utc::now(),
            process_id,
            stream,
            content: String::from_utf8_lossy(&line).into_owned(),
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::AsyncWriteExt;

    #[test]
    fn script_language_and_extension_are_strict_allowlists() {
        assert_eq!(language_for_name("python").unwrap().extension, "py");
        assert!(language_for_name("python3").is_none());
        assert_eq!(language_for_extension("ps1").unwrap().program, "powershell");
        assert_eq!(language_for_extension("go").unwrap().prefix_args, &["run"]);
        assert!(language_for_extension("txt").is_none());
    }

    #[tokio::test]
    async fn oversized_script_line_emits_a_limit_signal() {
        let (mut writer, reader) = tokio::io::duplex(128 * 1024);
        let (tx, mut rx) = broadcast::channel(4);
        let task = tokio::spawn(forward_script_output(
            BufReader::new(reader),
            LogStream::Stdout,
            tx,
            uuid::Uuid::nil(),
            Arc::new(AtomicUsize::new(0)),
        ));
        writer.write_all(&vec![b'x'; 64 * 1024 + 1]).await.unwrap();
        drop(writer);
        let event = rx.recv().await.unwrap();
        assert_eq!(event.content, "\0__output_limit__\0");
        task.await.unwrap();
    }
}
