/// Daemon — background process that:
///   1. Builds the initial index
///   2. Watches for FS changes via notify (inotify/FSEvents/ReadDirectoryChangesW)
///   3. Applies incremental updates to the index
///   4. Serves queries over a Unix socket (or named pipe on Windows)
///
/// IPC protocol (newline-delimited JSON over the socket):
///   Client sends: {"kind":"query", ...Query fields...}
///   Client sends: {"kind":"status"}
///   Client sends: {"kind":"rebuild"}
///   Server responds with a single JSON line per request.

use crate::bloom::{rebuild_bloom, BloomSet};
use crate::builder::build_index;
use crate::columns::ColumnarIndex;
use crate::query::{run_query, Query, QueryResult};
use notify::{Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};
use std::time::UNIX_EPOCH;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::UnixListener;
use tokio::sync::mpsc;

// ── IPC message types ─────────────────────────────────────────────────────────

#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Request {
    Query(Query),
    Status,
    Rebuild,
}

#[derive(Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Response {
    QueryResult(QueryResult),
    Status(DaemonStatus),
    Rebuilding,
    Error { message: String },
}

#[derive(Serialize)]
pub struct DaemonStatus {
    pub root: String,
    pub indexed_files: usize,
    pub built_at: u64,
    pub watching: bool,
    pub socket_path: String,
}

// ── Shared state ──────────────────────────────────────────────────────────────

struct SharedState {
    index: ColumnarIndex,
    bloom: BloomSet,
}

// ── Socket path ───────────────────────────────────────────────────────────────

pub fn socket_path(root: &Path) -> PathBuf {
    // Hash the root path to get a unique socket name per watched directory
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    let mut h = DefaultHasher::new();
    root.hash(&mut h);
    let hash = h.finish();

    #[cfg(unix)]
    return std::env::temp_dir().join(format!("idx-{:x}.sock", hash));

    #[cfg(windows)]
    return PathBuf::from(format!(r"\\.\pipe\idx-{:x}", hash));
}

// ── Daemon entry point ────────────────────────────────────────────────────────

pub async fn run_daemon(root: PathBuf, respect_gitignore: bool) {
    eprintln!("[idx] Building initial index for {} ...", root.display());

    let build = build_index(&root, respect_gitignore);
    eprintln!(
        "[idx] Indexed {} entries in {}ms",
        build.index.len, build.duration_ms
    );

    let state = Arc::new(RwLock::new(SharedState {
        index: build.index,
        bloom: build.bloom,
    }));

    let sock_path = socket_path(&root);

    // Remove stale socket
    let _ = std::fs::remove_file(&sock_path);

    let listener = UnixListener::bind(&sock_path).expect("failed to bind socket");
    eprintln!("[idx] Listening on {}", sock_path.display());

    // ── File watcher ─────────────────────────────────────────────────────────
    let (tx, mut rx) = mpsc::channel::<notify::Result<Event>>(256);
    let root_clone = root.clone();

    let mut watcher = RecommendedWatcher::new(
        move |res| { let _ = tx.blocking_send(res); },
        notify::Config::default(),
    ).expect("watcher init failed");

    watcher.watch(&root, RecursiveMode::Recursive).expect("watch failed");

    let state_for_watcher = state.clone();
    let root_for_rebuild = root.clone();

    // Watcher task — debounces events and triggers incremental updates
    tokio::spawn(async move {
        let mut pending: Vec<PathBuf> = Vec::new();
        let mut last_event = tokio::time::Instant::now();
        let debounce = tokio::time::Duration::from_millis(300);

        loop {
            tokio::select! {
                Some(Ok(event)) = rx.recv() => {
                    match event.kind {
                        EventKind::Create(_) | EventKind::Modify(_) | EventKind::Remove(_) => {
                            pending.extend(event.paths);
                            last_event = tokio::time::Instant::now();
                        }
                        _ => {}
                    }
                }
                _ = tokio::time::sleep(debounce) => {
                    if !pending.is_empty()
                        && last_event.elapsed() >= debounce
                    {
                        let changed: Vec<PathBuf> = pending.drain(..).collect();
                        apply_incremental_updates(
                            &state_for_watcher,
                            &root_for_rebuild,
                            &changed,
                            respect_gitignore,
                        ).await;
                    }
                }
            }
        }
    });

    // ── Accept loop ───────────────────────────────────────────────────────────
    loop {
        let (stream, _) = match listener.accept().await {
            Ok(s) => s,
            Err(e) => { eprintln!("[idx] accept error: {}", e); continue; }
        };

        let state = state.clone();
        let sock_str = sock_path.to_string_lossy().to_string();
        let root_str = root.to_string_lossy().to_string();

        tokio::spawn(async move {
            handle_client(stream, state, sock_str, root_str).await;
        });
    }
}

async fn handle_client(
    stream: tokio::net::UnixStream,
    state: Arc<RwLock<SharedState>>,
    sock_path: String,
    root: String,
) {
    let (reader, mut writer) = stream.into_split();
    let mut lines = BufReader::new(reader).lines();

    while let Ok(Some(line)) = lines.next_line().await {
        let response = match serde_json::from_str::<Request>(&line) {
            Ok(Request::Query(q)) => {
                let st = state.read().unwrap();
                let result = run_query(&st.index, &st.bloom, &q);
                Response::QueryResult(result)
            }
            Ok(Request::Status) => {
                let st = state.read().unwrap();
                Response::Status(DaemonStatus {
                    root: root.clone(),
                    indexed_files: st.index.len,
                    built_at: st.index.built_at,
                    watching: true,
                    socket_path: sock_path.clone(),
                })
            }
            Ok(Request::Rebuild) => {
                // Trigger a full rebuild in the background
                let state_clone = state.clone();
                let root_clone = root.clone();
                tokio::spawn(async move {
                    let build = tokio::task::spawn_blocking(move || {
                        build_index(Path::new(&root_clone), true)
                    }).await.unwrap();

                    let mut st = state_clone.write().unwrap();
                    st.index = build.index;
                    st.bloom = build.bloom;
                    eprintln!("[idx] Rebuild complete: {} entries", st.index.len);
                });
                Response::Rebuilding
            }
            Err(e) => Response::Error { message: e.to_string() },
        };

        let mut json = serde_json::to_string(&response).unwrap();
        json.push('\n');
        if writer.write_all(json.as_bytes()).await.is_err() { break; }
    }
}

/// Apply incremental updates for a set of changed paths.
/// For small changesets this is much faster than a full rebuild.
async fn apply_incremental_updates(
    state: &Arc<RwLock<SharedState>>,
    root: &Path,
    changed: &[PathBuf],
    respect_gitignore: bool,
) {
    // For simplicity: if > 100 paths changed, do a full rebuild.
    // For small changes: update individual rows.
    if changed.len() > 100 {
        let root = root.to_path_buf();
        let build = tokio::task::spawn_blocking(move || {
            build_index(&root, respect_gitignore)
        }).await.unwrap();

        let mut st = state.write().unwrap();
        st.index = build.index;
        st.bloom = build.bloom;
        eprintln!("[idx] Full rebuild: {} entries", st.index.len);
        return;
    }

    // Incremental: update or remove affected rows, add new ones
    let mut st = state.write().unwrap();

    for path in changed {
        let path_str = path.to_string_lossy().to_string();

        if path.exists() {
            // Update or insert
            if let Ok(meta) = path.metadata() {
                let size = meta.len();
                let mtime = meta.modified()
                    .ok()
                    .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
                    .map(|d| d.as_secs())
                    .unwrap_or(0);
                let ext = path.extension().and_then(|e| e.to_str());
                let is_dir = meta.is_dir();

                // Find existing row or push new one
                if let Some(pos) = st.index.paths.iter().position(|p| p == &path_str) {
                    st.index.sizes[pos] = size;
                    st.index.mtimes[pos] = mtime;
                } else {
                    st.index.push(
                        path_str.clone(),
                        size,
                        mtime,
                        ext,
                        crate::columns::GitStatus::Unknown,
                        is_dir,
                    );
                    st.bloom.insert(&path_str);
                }
            }
        } else {
            // File was deleted — remove the row
            if let Some(pos) = st.index.paths.iter().position(|p| p == &path_str) {
                st.index.paths.remove(pos);
                st.index.sizes.remove(pos);
                st.index.mtimes.remove(pos);
                st.index.ext_ids.remove(pos);
                st.index.git_status.remove(pos);
                st.index.dir_flags.remove(pos);
                st.index.len -= 1;
                // Note: bloom filter can't remove — false positives are tolerated
            }
        }
    }

    eprintln!("[idx] Incremental update: {} paths changed", changed.len());
}
