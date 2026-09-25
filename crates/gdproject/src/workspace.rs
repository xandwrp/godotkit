//! Where gdkit is allowed to write: `.godot/gdkit/**` inside the project, scratch
//! copies in the temp dir, and brand-new files published with create-new semantics.
//!
//! Linux x86/x86-64 uses descriptor-relative, no-follow traversal and pinned source files
//! (requiring `/proc/self/fd`). Replacing a pathname with a symlink cannot redirect
//! an in-flight read or write. Other platforms perform best-effort symlink checks,
//! not race-resistant traversal. No platform provides a snapshot against concurrent
//! content edits or protects against an attacker moving an already-open directory.
//! Staging files must be exclusively owned by the caller until publication returns.
//!
//! The project root is canonical ([`Project::root`]): the user-chosen root and
//! its ancestors are trusted, and symlink refusal applies to entries inside it.
//!
//! Scratch copies (`<temp>/gdkit-<unix_ms>-<pid>-<n>`) and rename-fallback
//! staging dirs (`.gdkit-publish-<pid>-<n>`) left by a killed process are swept
//! when a sibling of the same kind is created (Unix only): only exact-pattern
//! directories (never links) owned by the effective uid whose pid is gone.
//!
//! # Tests (`tests/workspace.rs`, plus private `[unit]` tests in this file)
//! - `open_requires_a_project_and_creates_state_dir_lazily`
//! - `non_canonical_roots_are_trusted_but_links_inside_are_not`
//! - `lock_is_exclusive_across_processes_and_released_on_drop`
//! - `isolated_copy_full_excludes_dot_godot_dot_git_and_refuses_symlinks`
//! - `copies_skip_gdignored_directories_entirely`
//! - `isolated_copy_slice_rejects_dot_dot_absolute_and_dot_godot_and_keeps_relative_layout`
//! - `isolated_copy_slice_brings_uid_and_import_sidecars`
//! - `normalize_slice_path_is_the_shared_slice_rule`
//! - `isolated_copy_is_removed_on_drop`
//! - `artifact_dir_names_are_unique_and_sortable`
//! - `publish_new_file_is_atomic_and_never_overwrites`
//! - `publish_new_file_succeeds_when_only_staging_cleanup_fails`
//! - `publish_new_file_falls_back_from_hard_link_to_rename_on_filesystems_without_links` `[unit]`
//! - `allocation_sweeps_only_dead_owned_scratch_dirs` `[unit]`
//! - `rename_fallback_sweeps_dead_staging_dirs` `[unit]`
//! - `slice_copy_refuses_scratch_inside_selected_source` `[unit]`

use std::ffi::OsStr;
#[cfg(not(all(target_os = "linux", any(target_arch = "x86", target_arch = "x86_64"))))]
use std::fs::OpenOptions;
use std::fs::{self, File};
use std::io::{self, Write};
use std::path::{Component, Path, PathBuf};
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

use gdview::Project;

use crate::{Error, Result};

pub struct Workspace {
    project: Project,
    state_dir: PathBuf,
}

impl Workspace {
    /// Opens the project at `root` (no discovery; the CLI decides that).
    /// The root is canonicalized, so `..`, a symlinked checkout, or a symlinked
    /// ancestor all work; `root()` reports the canonical path.
    pub fn open(root: &Path) -> Result<Self> {
        let project = Project::open(root)?;
        let state_dir = project.root().join(".godot").join("gdkit");
        Ok(Self { project, state_dir })
    }
    pub fn project(&self) -> &Project {
        &self.project
    }
    pub fn root(&self) -> &Path {
        self.project.root()
    }
    /// `<root>/.godot/gdkit`
    pub fn state_dir(&self) -> &Path {
        &self.state_dir
    }
    pub fn probe_cache_path(&self) -> PathBuf {
        self.state_dir.join("engine-probe.json")
    }
    pub fn api_cache_path(&self) -> PathBuf {
        self.state_dir.join("api-index.json")
    }
    /// Exclusive, nonblocking lock for operations that write the real `.godot`.
    pub fn lock(&self) -> Result<Lock> {
        let path = self.state_dir.join("lock");
        #[cfg(all(target_os = "linux", any(target_arch = "x86", target_arch = "x86_64")))]
        let file = linux::lock_file(&path).map_err(|e| io_error(&path, e))?;
        #[cfg(not(all(target_os = "linux", any(target_arch = "x86", target_arch = "x86_64"))))]
        let file = {
            ensure_directory(&self.state_dir)?;
            refuse_existing_nonfile(&path)?;
            OpenOptions::new()
                .read(true)
                .write(true)
                .create(true)
                .truncate(false)
                .open(&path)
                .map_err(|e| io_error(&path, e))?
        };
        match file.try_lock() {
            Ok(()) => Ok(Lock { file }),
            Err(std::fs::TryLockError::WouldBlock) => Err(Error::Locked(path)),
            Err(std::fs::TryLockError::Error(e)) => Err(io_error(&path, e)),
        }
    }
    /// New `<state>/artifacts/<kind>/<unix_ms>-<pid>-<n>/`.
    pub fn new_artifact_dir(&self, kind: &str) -> Result<ArtifactDir> {
        validate_relative(Path::new(kind), false)?;
        if Path::new(kind).components().count() != 1 {
            return Err(Error::Invalid("artifact kind must be a single name".into()));
        }
        let parent = self.state_dir.join("artifacts").join(kind);
        ensure_directory(&parent)?;
        Ok(ArtifactDir {
            path: unique_directory(&parent, "")?,
        })
    }
}

pub struct Lock {
    file: File,
}

impl Drop for Lock {
    fn drop(&mut self) {
        // Keep the inode: unlinking a lock file lets newcomers lock a different file.
        let _ = self.file.unlock();
    }
}

pub struct ArtifactDir {
    pub path: PathBuf,
}

impl ArtifactDir {
    /// Writes a new artifact without replacement. See the module-level
    /// platform-specific guarantees for symlink traversal.
    pub fn write(&self, name: &str, bytes: &[u8]) -> Result<PathBuf> {
        let relative = Path::new(name);
        validate_relative(relative, false)?;
        let path = self.path.join(relative);
        ensure_directory(path.parent().expect("joined relative path has a parent"))?;
        write_new(&path, bytes)?;
        Ok(path)
    }
}

const SCRATCH_PREFIX: &str = "gdkit-";

/// A disposable copy of the project in the temp dir, removed on drop.
///
/// Drop removes only the directory this type allocated (the private
/// `allocated`), never whatever `path` holds; the private field also makes the
/// type impossible to construct outside this module.
pub struct IsolatedCopy {
    allocated: PathBuf,
}

impl IsolatedCopy {
    /// The scratch project root: canonical, directly under the system temp dir.
    pub fn path(&self) -> &Path {
        &self.allocated
    }

    fn new(allocated: PathBuf) -> Self {
        Self { allocated }
    }

    fn allocate() -> Result<Self> {
        // Canonicalize the system temp root (which itself may be an OS-managed link).
        Self::allocate_in(&std::env::temp_dir(), None)
    }

    fn allocate_in(temp: &Path, project: Option<&Project>) -> Result<Self> {
        let temp = fs::canonicalize(temp).map_err(|e| io_error(temp, e))?;
        if let Some(project) = project {
            let root = fs::canonicalize(project.root()).map_err(|e| io_error(project.root(), e))?;
            // Conservatively reject the whole project, even unselected/excluded
            // subtrees: scratch must never be created in the authored source.
            if temp.starts_with(&root) {
                return Err(Error::Invalid(format!(
                    "scratch root {} is inside project {}",
                    temp.display(),
                    root.display()
                )));
            }
        }
        sweep_stale_scratch(&temp);
        Ok(Self::new(unique_directory(&temp, SCRATCH_PREFIX)?))
    }

    /// A bare `project.godot` with `config_version=5`.
    pub fn empty() -> Result<Self> {
        let copy = Self::allocate()?;
        write_new(&copy.path().join("project.godot"), b"config_version=5\n")?;
        Ok(copy)
    }
    /// Everything except `.godot` and `.git` entries at any depth (see
    /// [`normalize_slice_path`] for the name match) and directories holding a
    /// `.gdignore` file, which Godot ignores entirely. Excluded entries are not
    /// visited; all other symlinks and special files fail.
    /// Rejects a canonical system temp root inside the project before allocating.
    pub fn full(project: &Project) -> Result<Self> {
        Self::full_in(project, &std::env::temp_dir())
    }

    fn full_in(project: &Project, temp: &Path) -> Result<Self> {
        check_directory(project.root())?;
        let copy = Self::allocate_in(temp, Some(project))?;
        copy_tree(project.root(), copy.path(), copy.path(), false)?;
        Ok(copy)
    }
    /// Only selected relative files/directories, retaining their layout, plus a
    /// minimal `project.godot` unless that file is explicitly selected.
    /// A selected file brings its existing Godot sidecars (`<file>.uid`,
    /// `<file>.import`). Selected directories are filtered like `full`; one that
    /// itself holds `.gdignore` contributes nothing. Selections must already be
    /// normalized (no `.` components; see [`normalize_slice_path`]).
    /// Like `full`, requires the canonical system temp root outside the project.
    pub fn slice(project: &Project, selections: &[PathBuf]) -> Result<Self> {
        Self::slice_in(project, selections, &std::env::temp_dir())
    }

    fn slice_in(project: &Project, selections: &[PathBuf], temp: &Path) -> Result<Self> {
        check_directory(project.root())?;
        let mut paths = Vec::new();
        for selection in selections {
            validate_relative(selection, true)?;
            let source = project.root().join(selection);
            check_ancestors(&source)?;
            paths.push(selection.clone());
            if fs::symlink_metadata(&source)
                .map_err(|e| io_error(&source, e))?
                .is_file()
            {
                for suffix in [".uid", ".import"] {
                    let mut sidecar = selection.as_os_str().to_owned();
                    sidecar.push(suffix);
                    let sidecar = PathBuf::from(sidecar);
                    let path = project.root().join(&sidecar);
                    // Present sidecars are copied (and refused if links) like
                    // any selection; absent ones are simply not needed.
                    match fs::symlink_metadata(&path) {
                        Ok(_) => paths.push(sidecar),
                        Err(e) if e.kind() == io::ErrorKind::NotFound => {}
                        Err(e) => return Err(io_error(&path, e)),
                    }
                }
            }
        }
        // Ancestors sort before descendants; copy each entry only once.
        paths.sort();
        paths.dedup();
        let mut selected: Vec<PathBuf> = Vec::new();
        for path in paths {
            if !selected.iter().any(|parent| path.starts_with(parent)) {
                selected.push(path);
            }
        }
        let copy = Self::allocate_in(temp, Some(project))?;
        for path in &selected {
            let destination = copy.path().join(path);
            ensure_directory(destination.parent().expect("selected path has parent"))?;
            // The destination does not exist yet; guard against the scratch
            // root itself turning up inside the copied source instead.
            copy_tree(&project.root().join(path), &destination, copy.path(), true)?;
        }
        if !selected
            .iter()
            .any(|path| path == Path::new("project.godot"))
        {
            write_new(&copy.path().join("project.godot"), b"config_version=5\n")?;
        }
        Ok(copy)
    }
}

impl Drop for IsolatedCopy {
    fn drop(&mut self) {
        #[cfg(all(target_os = "linux", any(target_arch = "x86", target_arch = "x86_64")))]
        let _ = linux::remove_tree(&self.allocated);
        #[cfg(not(all(target_os = "linux", any(target_arch = "x86", target_arch = "x86_64"))))]
        let _ = fs::remove_dir_all(&self.allocated);
    }
}

fn io_error(path: &Path, source: io::Error) -> Error {
    Error::Io {
        path: path.to_owned(),
        source,
    }
}

fn invalid_path(path: &Path) -> Error {
    Error::Invalid(format!("unsafe workspace path: {}", path.display()))
}

/// The one rule for user-supplied slice entries (e.g. `--slice`), shared by
/// [`IsolatedCopy::slice`] and its callers. `.` components are dropped; what
/// remains must be relative, made only of normal components (no `..`, root, or
/// drive prefix), contain no backslash (rejected everywhere so manifests stay
/// portable) and, on Windows, no `:`. No component may be `.godot` or `.git`,
/// compared ASCII case-insensitively (a case-insensitive filesystem resolves
/// `.GODOT` to the import cache) and, on Windows, ignoring trailing dots and
/// spaces (which Win32 strips).
///
/// Returns the entry `/`-joined; `""` means the whole project (`.`, `./`).
/// An empty input is rejected. Pass a non-empty result to `IsolatedCopy::slice`.
pub fn normalize_slice_path(path: &str) -> Result<String> {
    let invalid = || invalid_path(Path::new(path));
    // Checked on the raw text: on Windows `\` would otherwise split silently.
    if path.is_empty() || path.contains('\\') {
        return Err(invalid());
    }
    let mut kept = Vec::new();
    for component in Path::new(path).components() {
        match component {
            Component::CurDir => {}
            Component::Normal(name) => kept.push(name.to_str().ok_or_else(invalid)?),
            _ => return Err(invalid()),
        }
    }
    let normalized = kept.join("/");
    if !normalized.is_empty() {
        validate_relative(Path::new(&normalized), true)?;
    }
    Ok(normalized)
}

/// `.godot` (engine state/import cache) and `.git`, matched as documented on
/// [`normalize_slice_path`].
fn is_excluded_name(name: &OsStr) -> bool {
    let Some(name) = name.to_str() else {
        return false;
    };
    #[cfg(windows)]
    let name = name.trim_end_matches(['.', ' ']);
    name.eq_ignore_ascii_case(".godot") || name.eq_ignore_ascii_case(".git")
}

fn validate_relative(path: &Path, exclude_state: bool) -> Result<()> {
    // Reject backslashes on Unix too, so manifests are portable. `:` is an
    // ordinary filename byte on Unix but a drive/stream separator on Windows.
    let text = path.as_os_str().to_string_lossy();
    if text.is_empty() || text.contains('\\') || (cfg!(windows) && text.contains(':')) {
        return Err(invalid_path(path));
    }
    for component in path.components() {
        let Component::Normal(name) = component else {
            return Err(invalid_path(path));
        };
        if exclude_state && is_excluded_name(name) {
            return Err(invalid_path(path));
        }
    }
    Ok(())
}

/// `gdkit-<20 digits>-<10 digit pid>-<20 digits>`, exactly as `unique_directory` names scratch.
#[cfg_attr(not(unix), allow(dead_code))]
fn scratch_pid(name: &str) -> Option<u32> {
    let mut parts = name.strip_prefix(SCRATCH_PREFIX)?.split('-');
    let fields = [parts.next()?, parts.next()?, parts.next()?];
    let digits = |field: &str, len| field.len() == len && field.bytes().all(|b| b.is_ascii_digit());
    if parts.next().is_some()
        || !digits(fields[0], 20)
        || !digits(fields[1], 10)
        || !digits(fields[2], 20)
    {
        return None;
    }
    fields[1].parse().ok()
}

/// `.gdkit-publish-<pid>-<n>`, exactly as the Linux rename fallback names staging.
#[cfg_attr(
    not(all(target_os = "linux", any(target_arch = "x86", target_arch = "x86_64"))),
    allow(dead_code)
)]
fn staging_pid(name: &str) -> Option<u32> {
    let (pid, counter) = name.strip_prefix(".gdkit-publish-")?.split_once('-')?;
    let digits = |field: &str, max| {
        !field.is_empty() && field.len() <= max && field.bytes().all(|b| b.is_ascii_digit())
    };
    if !digits(pid, 10) || !digits(counter, 20) {
        return None;
    }
    pid.parse().ok()
}

#[cfg(unix)]
mod liveness {
    use std::ffi::c_int;
    use std::fs::Metadata;
    use std::io;
    use std::os::unix::fs::MetadataExt;

    unsafe extern "C" {
        fn kill(pid: c_int, signal: c_int) -> c_int;
        fn geteuid() -> u32;
    }

    // Same value on Linux, macOS, and the BSDs.
    const ESRCH: i32 = 3;

    /// True only when the process is known not to exist. EPERM (someone
    /// else's live process), zombies, and odd pids all count as alive.
    pub(super) fn definitely_dead(pid: u32) -> bool {
        let Ok(raw) = c_int::try_from(pid) else {
            return false;
        };
        if raw <= 0 || pid == std::process::id() {
            return false;
        }
        // Signal 0 only checks for existence and permission; nothing is sent.
        (unsafe { kill(raw, 0) }) == -1 && io::Error::last_os_error().raw_os_error() == Some(ESRCH)
    }

    /// A no-follow `metadata` of a leftover this user may delete: a real
    /// directory (not a link) owned by our effective uid, from a dead pid.
    pub(super) fn is_stale(metadata: &Metadata, pid: u32) -> bool {
        metadata.is_dir() && metadata.uid() == unsafe { geteuid() } && definitely_dead(pid)
    }
}

/// Best effort: a failed sweep never blocks allocating a new scratch copy.
#[cfg(unix)]
fn sweep_stale_scratch(temp: &Path) {
    let Ok(entries) = fs::read_dir(temp) else {
        return;
    };
    for entry in entries.flatten() {
        let name = entry.file_name();
        let Some(pid) = name.to_str().and_then(scratch_pid) else {
            continue;
        };
        let path = temp.join(&name);
        match fs::symlink_metadata(&path) {
            Ok(metadata) if liveness::is_stale(&metadata, pid) => {}
            _ => continue,
        }
        // Neither removal follows links below (or at) the named entry.
        #[cfg(all(target_os = "linux", any(target_arch = "x86", target_arch = "x86_64")))]
        let _ = linux::remove_tree(&path);
        #[cfg(not(all(target_os = "linux", any(target_arch = "x86", target_arch = "x86_64"))))]
        let _ = fs::remove_dir_all(&path);
    }
}

/// Without a portable liveness probe, leftovers are not swept.
#[cfg(not(unix))]
fn sweep_stale_scratch(_temp: &Path) {}

/// Check every existing component, not just the final entry.
fn check_ancestors(path: &Path) -> Result<()> {
    for ancestor in path.ancestors() {
        if ancestor.as_os_str().is_empty() {
            continue;
        }
        let metadata = fs::symlink_metadata(ancestor).map_err(|e| io_error(ancestor, e))?;
        if metadata.file_type().is_symlink() {
            return Err(invalid_path(ancestor));
        }
        if ancestor != path && !metadata.is_dir() {
            return Err(invalid_path(ancestor));
        }
    }
    Ok(())
}

fn check_directory(path: &Path) -> Result<()> {
    check_ancestors(path)?;
    if !fs::symlink_metadata(path)
        .map_err(|e| io_error(path, e))?
        .is_dir()
    {
        return Err(invalid_path(path));
    }
    Ok(())
}

#[cfg(all(target_os = "linux", any(target_arch = "x86", target_arch = "x86_64")))]
fn ensure_directory(path: &Path) -> Result<()> {
    linux::Dir::open(path, true)
        .map(|_| ())
        .map_err(|e| io_error(path, e))
}

#[cfg(not(all(target_os = "linux", any(target_arch = "x86", target_arch = "x86_64"))))]
fn ensure_directory(path: &Path) -> Result<()> {
    if path.as_os_str().is_empty() {
        return Ok(());
    }
    match fs::symlink_metadata(path) {
        Ok(_) => check_directory(path),
        Err(e) if e.kind() == io::ErrorKind::NotFound => {
            if let Some(parent) = path.parent() {
                ensure_directory(parent)?;
            }
            match fs::create_dir(path) {
                Ok(()) => Ok(()),
                Err(e) if e.kind() == io::ErrorKind::AlreadyExists => check_directory(path),
                Err(e) => Err(io_error(path, e)),
            }
        }
        Err(e) => Err(io_error(path, e)),
    }
}

fn refuse_existing_nonfile(path: &Path) -> Result<()> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.is_file() => Ok(()),
        Ok(_) => Err(invalid_path(path)),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(io_error(path, e)),
    }
}

#[cfg(all(target_os = "linux", any(target_arch = "x86", target_arch = "x86_64")))]
fn write_new(path: &Path, bytes: &[u8]) -> Result<()> {
    linux::write_new(path, bytes).map_err(|e| io_error(path, e))
}

#[cfg(not(all(target_os = "linux", any(target_arch = "x86", target_arch = "x86_64"))))]
fn write_new(path: &Path, bytes: &[u8]) -> Result<()> {
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(|e| io_error(path, e))?;
    if let Err(e) = file.write_all(bytes) {
        drop(file);
        let _ = fs::remove_file(path);
        return Err(io_error(path, e));
    }
    Ok(())
}

fn unique_directory(parent: &Path, prefix: &str) -> Result<PathBuf> {
    static COUNTER: Mutex<(u128, u64)> = Mutex::new((0, 0));
    #[cfg(all(target_os = "linux", any(target_arch = "x86", target_arch = "x86_64")))]
    let directory = linux::Dir::open(parent, false).map_err(|e| io_error(parent, e))?;
    loop {
        let name = {
            let mut state = COUNTER.lock().unwrap_or_else(|e| e.into_inner());
            let now = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis();
            state.0 = state.0.max(now);
            state.1 += 1;
            format!(
                "{prefix}{:020}-{:010}-{:020}",
                state.0,
                std::process::id(),
                state.1
            )
        };
        let path = parent.join(name);
        #[cfg(all(target_os = "linux", any(target_arch = "x86", target_arch = "x86_64")))]
        let result = directory.mkdir(path.file_name().expect("generated name"));
        #[cfg(not(all(target_os = "linux", any(target_arch = "x86", target_arch = "x86_64"))))]
        let result = {
            let mut builder = fs::DirBuilder::new();
            #[cfg(unix)]
            {
                use std::os::unix::fs::DirBuilderExt;
                builder.mode(0o700);
            }
            builder.create(&path)
        };
        match result {
            Ok(()) => return Ok(path),
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(io_error(&path, e)),
        }
    }
}

const GDIGNORE: &str = ".gdignore";

/// Copies `source` to `destination` (which must not exist unless it is
/// `scratch`), failing if the `scratch` root is found inside the copied tree.
/// With `skip_ignored`, a `source` directory holding `.gdignore` is skipped;
/// nested ones always are.
#[cfg(all(target_os = "linux", any(target_arch = "x86", target_arch = "x86_64")))]
fn copy_tree(source: &Path, destination: &Path, scratch: &Path, skip_ignored: bool) -> Result<()> {
    linux::copy_tree(source, destination, scratch, skip_ignored).map_err(|e| io_error(source, e))
}

fn copy_permissions(output: &File, metadata: &fs::Metadata) -> io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        output.set_permissions(fs::Permissions::from_mode(
            (metadata.permissions().mode() & 0o777) | 0o600,
        ))?;
    }
    Ok(())
}

#[cfg(not(all(target_os = "linux", any(target_arch = "x86", target_arch = "x86_64"))))]
fn copy_tree(source: &Path, destination: &Path, scratch: &Path, skip_ignored: bool) -> Result<()> {
    // Best effort (paths, not pinned inodes): refuse a scratch root that
    // resolves inside the source before visiting anything.
    let canonical = |path: &Path| fs::canonicalize(path).map_err(|e| io_error(path, e));
    if canonical(scratch)?.starts_with(canonical(source)?) {
        return Err(Error::Invalid(format!(
            "scratch {} overlaps copied source {}",
            scratch.display(),
            source.display()
        )));
    }
    copy_entry(source, destination, skip_ignored)
}

#[cfg(not(all(target_os = "linux", any(target_arch = "x86", target_arch = "x86_64"))))]
fn copy_entry(source: &Path, destination: &Path, skip_ignored: bool) -> Result<()> {
    let metadata = fs::symlink_metadata(source).map_err(|e| io_error(source, e))?;
    if metadata.is_dir() {
        let mut entries = fs::read_dir(source)
            .map_err(|e| io_error(source, e))?
            .collect::<io::Result<Vec<_>>>()
            .map_err(|e| io_error(source, e))?;
        // Godot skips the whole directory; so do we, without inspecting it.
        if skip_ignored && entries.iter().any(|entry| entry.file_name() == GDIGNORE) {
            return Ok(());
        }
        ensure_directory(destination)?;
        entries.sort_by_key(|entry| entry.file_name());
        for entry in entries {
            if is_excluded_name(&entry.file_name()) {
                continue;
            }
            copy_entry(&entry.path(), &destination.join(entry.file_name()), true)?;
        }
        Ok(())
    } else if metadata.is_file() {
        // Keep executable tools usable, but clear setuid/setgid/sticky and make
        // the scratch copy owner-readable/writable regardless of source mode.
        let mut input = File::open(source).map_err(|e| io_error(source, e))?;
        let mut output = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(destination)
            .map_err(|e| io_error(destination, e))?;
        io::copy(&mut input, &mut output).map_err(|e| io_error(destination, e))?;
        copy_permissions(&output, &metadata).map_err(|e| io_error(destination, e))?;
        Ok(())
    } else {
        Err(invalid_path(source))
    }
}

/// Atomically publishes `staged` at `destination`, never replacing an entry.
/// The staging file must be regular and on the destination filesystem. On
/// success it is consumed, best effort: once `destination` exists the call
/// succeeds even if removing the staging name then fails (it is left behind).
/// No partial destination is exposed on failure.
/// Filesystems without hard links use an OS no-replace rename; platforms without
/// that primitive fail closed rather than risk overwriting an authored file.
pub fn publish_new_file(staged: &Path, destination: &Path) -> Result<()> {
    publish_with(staged, destination, || Ok(()))
}

fn publish_with(
    staged: &Path,
    destination: &Path,
    before_link: impl FnOnce() -> io::Result<()>,
) -> Result<()> {
    #[cfg(all(target_os = "linux", any(target_arch = "x86", target_arch = "x86_64")))]
    {
        linux::publish(staged, destination, before_link).map_err(|e| io_error(destination, e))
    }
    #[cfg(not(all(target_os = "linux", any(target_arch = "x86", target_arch = "x86_64"))))]
    {
        check_ancestors(staged)?;
        if !fs::symlink_metadata(staged)
            .map_err(|e| io_error(staged, e))?
            .is_file()
        {
            return Err(invalid_path(staged));
        }
        let parent = destination
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or(Path::new("."));
        check_directory(parent)?;
        if destination.file_name().is_none() {
            return Err(invalid_path(destination));
        }
        match before_link().and_then(|()| fs::hard_link(staged, destination)) {
            Ok(()) => {
                // Published; a failed cleanup must not report the publication as failed.
                let _ = fs::remove_file(staged);
                Ok(())
            }
            Err(e)
                if matches!(
                    e.kind(),
                    io::ErrorKind::Unsupported | io::ErrorKind::PermissionDenied
                ) =>
            {
                rename_new(staged, destination).map_err(|e| io_error(destination, e))
            }
            Err(e) => Err(io_error(destination, e)),
        }
    }
}

#[cfg(all(target_os = "linux", any(target_arch = "x86", target_arch = "x86_64")))]
mod linux {
    use super::*;
    use std::ffi::{CString, OsStr, OsString, c_char, c_int};
    use std::os::fd::{AsRawFd, FromRawFd};
    use std::os::unix::ffi::OsStrExt;
    use std::os::unix::fs::MetadataExt;
    use std::sync::atomic::{AtomicU64, Ordering};

    // x86 Linux UAPI flags (not valid on all Linux architectures). No libc crate
    // is a direct dependency; keep the C surface
    // small, with File owning every successful openat descriptor.
    const RDONLY: c_int = 0;
    const WRONLY: c_int = 1;
    const RDWR: c_int = 2;
    const CREAT: c_int = 0o100;
    const EXCL: c_int = 0o200;
    const NONBLOCK: c_int = 0o4000;
    const DIRECTORY: c_int = 0o200000;
    const NOFOLLOW: c_int = 0o400000;
    const CLOEXEC: c_int = 0o2000000;
    const PATH: c_int = 0o10000000;
    const AT_FDCWD: c_int = -100;
    const AT_REMOVEDIR: c_int = 0x200;
    const AT_SYMLINK_FOLLOW: c_int = 0x400;

    unsafe extern "C" {
        fn openat(fd: c_int, path: *const c_char, flags: c_int, ...) -> c_int;
        fn mkdirat(fd: c_int, path: *const c_char, mode: u32) -> c_int;
        fn unlinkat(fd: c_int, path: *const c_char, flags: c_int) -> c_int;
        fn linkat(
            oldfd: c_int,
            old: *const c_char,
            newfd: c_int,
            new: *const c_char,
            flags: c_int,
        ) -> c_int;
        fn renameat2(
            oldfd: c_int,
            old: *const c_char,
            newfd: c_int,
            new: *const c_char,
            flags: u32,
        ) -> c_int;
    }

    fn c_name(name: &OsStr) -> io::Result<CString> {
        Ok(CString::new(name.as_bytes())?)
    }

    fn status(result: c_int) -> io::Result<()> {
        if result == 0 {
            Ok(())
        } else {
            Err(io::Error::last_os_error())
        }
    }

    fn open(fd: c_int, name: &OsStr, flags: c_int) -> io::Result<File> {
        let name = c_name(name)?;
        // The name lives through the call; the mode is used only with O_CREAT.
        let fd = unsafe { openat(fd, name.as_ptr(), flags | CLOEXEC, 0o600_u32) };
        if fd < 0 {
            Err(io::Error::last_os_error())
        } else {
            // Ownership transfers exactly once from openat into File.
            Ok(unsafe { File::from_raw_fd(fd) })
        }
    }

    fn unsafe_entry() -> io::Error {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "symlink, special file, or unsafe path component",
        )
    }

    fn proc_path(file: &File) -> PathBuf {
        PathBuf::from(format!("/proc/self/fd/{}", file.as_raw_fd()))
    }

    pub(super) struct Dir(File);

    impl Dir {
        pub(super) fn open(path: &Path, create: bool) -> io::Result<Self> {
            let start = if path.is_absolute() { "/" } else { "." };
            let mut dir = Self(open(
                AT_FDCWD,
                OsStr::new(start),
                RDONLY | DIRECTORY | NOFOLLOW,
            )?);
            for component in path.components() {
                match component {
                    Component::RootDir | Component::CurDir => {}
                    Component::Normal(name) => dir = dir.child(name, create)?,
                    _ => return Err(unsafe_entry()),
                }
            }
            Ok(dir)
        }

        fn child(&self, name: &OsStr, create: bool) -> io::Result<Self> {
            match open(self.0.as_raw_fd(), name, RDONLY | DIRECTORY | NOFOLLOW) {
                Ok(file) => Ok(Self(file)),
                Err(e) if create && e.kind() == io::ErrorKind::NotFound => {
                    match self.mkdir(name) {
                        Ok(()) => {}
                        Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {}
                        Err(e) => return Err(e),
                    }
                    open(self.0.as_raw_fd(), name, RDONLY | DIRECTORY | NOFOLLOW).map(Self)
                }
                Err(e) => Err(e),
            }
        }

        pub(super) fn mkdir(&self, name: &OsStr) -> io::Result<()> {
            let name = c_name(name)?;
            // Only a single component is passed by callers; the parent is pinned.
            status(unsafe { mkdirat(self.0.as_raw_fd(), name.as_ptr(), 0o700) })
        }

        fn new_file(&self, name: &OsStr) -> io::Result<File> {
            open(self.0.as_raw_fd(), name, WRONLY | CREAT | EXCL | NOFOLLOW)
        }

        fn unlink(&self, name: &OsStr, directory: bool) -> io::Result<()> {
            let name = c_name(name)?;
            status(unsafe {
                unlinkat(
                    self.0.as_raw_fd(),
                    name.as_ptr(),
                    if directory { AT_REMOVEDIR } else { 0 },
                )
            })
        }

        fn node(&self, name: &OsStr) -> io::Result<File> {
            self.node_with(name, || {})
        }

        fn node_with(&self, name: &OsStr, after_pin: impl FnOnce()) -> io::Result<File> {
            // O_PATH never opens a device or blocks on a FIFO. Validate the pinned
            // inode, then reopen that same inode, not the now-racy original name.
            let pin = open(self.0.as_raw_fd(), name, PATH | NOFOLLOW)?;
            let metadata = pin.metadata()?;
            if !metadata.is_file() && !metadata.is_dir() {
                return Err(unsafe_entry());
            }
            after_pin();
            let flags = if metadata.is_dir() {
                RDONLY | DIRECTORY
            } else {
                RDONLY | NONBLOCK
            };
            // This is an intentional procfs magic-link traversal to our own live
            // descriptor, not traversal through any user-controlled symlink.
            open(AT_FDCWD, proc_path(&pin).as_os_str(), flags)
        }
    }

    fn parent(path: &Path, create: bool) -> io::Result<(Dir, OsString)> {
        let name = path.file_name().ok_or_else(unsafe_entry)?.to_os_string();
        let dir = Dir::open(path.parent().unwrap_or(Path::new(".")), create)?;
        Ok((dir, name))
    }

    pub(super) fn lock_file(path: &Path) -> io::Result<File> {
        let (dir, name) = parent(path, true)?;
        let file = open(dir.0.as_raw_fd(), &name, RDWR | CREAT | NOFOLLOW | NONBLOCK)?;
        if !file.metadata()?.is_file() {
            return Err(unsafe_entry());
        }
        Ok(file)
    }

    pub(super) fn remove_tree(path: &Path) -> io::Result<()> {
        let (dir, name) = parent(path, false)?;
        // std's remove_dir_all does not follow descendant links on Linux; anchor
        // its initial lookup too, rather than resolving the original parent again.
        fs::remove_dir_all(proc_path(&dir.0).join(name))
    }

    pub(super) fn write_new(path: &Path, bytes: &[u8]) -> io::Result<()> {
        let (dir, name) = parent(path, true)?;
        let mut file = dir.new_file(&name)?;
        if let Err(e) = file.write_all(bytes) {
            let _ = dir.unlink(&name, false);
            return Err(e);
        }
        Ok(())
    }

    fn identity(file: &File) -> io::Result<(u64, u64)> {
        let metadata = file.metadata()?;
        Ok((metadata.dev(), metadata.ino()))
    }

    pub(super) fn copy_tree(
        source: &Path,
        destination: &Path,
        scratch: &Path,
        skip_ignored: bool,
    ) -> io::Result<()> {
        // The scratch root always exists (a slice destination may not yet), so
        // its inode is what the traversal must never meet inside the source.
        let forbidden = identity(&Dir::open(scratch, false)?.0)?;
        let (source_dir, source_name) = parent(source, false)?;
        let source = source_dir.node(&source_name)?;
        let (destination_dir, destination_name) = parent(destination, true)?;
        copy_node(
            source,
            &destination_dir,
            &destination_name,
            Some(forbidden),
            skip_ignored,
        )
    }

    fn copy_node(
        mut source: File,
        destination: &Dir,
        name: &OsStr,
        forbidden: Option<(u64, u64)>,
        skip_ignored: bool,
    ) -> io::Result<()> {
        let metadata = source.metadata()?;
        // Also catch a scratch directory moved into the source during traversal.
        if forbidden == Some((metadata.dev(), metadata.ino())) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "scratch overlaps copied source",
            ));
        }
        if metadata.is_dir() {
            let source = Dir(source);
            // read_dir pins its directory stream. Entry paths are deliberately
            // ignored; every child is opened relative to the retained descriptor.
            let mut names = fs::read_dir(proc_path(&source.0))?
                .map(|entry| entry.map(|entry| entry.file_name()))
                .collect::<io::Result<Vec<_>>>()?;
            // Godot skips the whole directory; so do we, without inspecting it.
            if skip_ignored && names.iter().any(|name| name == GDIGNORE) {
                return Ok(());
            }
            let destination = destination.child(name, true)?;
            names.sort();
            for name in names {
                if is_excluded_name(&name) {
                    continue;
                }
                copy_node(source.node(&name)?, &destination, &name, forbidden, true)?;
            }
            Ok(())
        } else {
            let mut output = destination.new_file(name)?;
            io::copy(&mut source, &mut output)?;
            copy_permissions(&output, &metadata)
        }
    }

    // A private staging directory is necessary for rename fallback: renaming the
    // caller's original pathname after checking it could publish a substituted link.
    struct Staging<'a> {
        parent: &'a Dir,
        name: OsString,
        dir: Dir,
    }

    /// Best effort: removes `.gdkit-publish-*` dirs whose creator is gone.
    fn sweep_stale_staging(parent: &Dir) {
        let Ok(entries) = fs::read_dir(proc_path(&parent.0)) else {
            return;
        };
        for entry in entries.flatten() {
            let name = entry.file_name();
            let Some(pid) = name.to_str().and_then(staging_pid) else {
                continue;
            };
            // O_PATH|O_NOFOLLOW: a link here is inspected as a link, not followed.
            let Ok(pin) = open(parent.0.as_raw_fd(), &name, PATH | NOFOLLOW) else {
                continue;
            };
            if pin
                .metadata()
                .is_ok_and(|metadata| liveness::is_stale(&metadata, pid))
            {
                // Anchored at the pinned parent; descendants are never followed.
                let _ = fs::remove_dir_all(proc_path(&parent.0).join(&name));
            }
        }
    }

    impl<'a> Staging<'a> {
        fn new(parent: &'a Dir) -> io::Result<Self> {
            static NEXT: AtomicU64 = AtomicU64::new(0);
            sweep_stale_staging(parent);
            loop {
                let name = OsString::from(format!(
                    ".gdkit-publish-{}-{}",
                    std::process::id(),
                    NEXT.fetch_add(1, Ordering::Relaxed)
                ));
                match parent.mkdir(&name) {
                    Ok(()) => {
                        let dir = match parent.child(&name, false) {
                            Ok(dir) => dir,
                            Err(e) => {
                                let _ = parent.unlink(&name, true);
                                return Err(e);
                            }
                        };
                        return Ok(Self { parent, name, dir });
                    }
                    Err(e) if e.kind() == io::ErrorKind::AlreadyExists => continue,
                    Err(e) => return Err(e),
                }
            }
        }
    }

    impl Drop for Staging<'_> {
        fn drop(&mut self) {
            let _ = self.dir.unlink(OsStr::new("payload"), false);
            let _ = self.parent.unlink(&self.name, true);
        }
    }

    pub(super) fn publish(
        staged: &Path,
        destination: &Path,
        before_link: impl FnOnce() -> io::Result<()>,
    ) -> io::Result<()> {
        let (source_dir, source_name) = parent(staged, false)?;
        let mut source = source_dir.node(&source_name)?;
        if !source.metadata()?.is_file() {
            return Err(unsafe_entry());
        }
        let (destination_dir, destination_name) = parent(destination, false)?;
        let pinned_name = c_name(proc_path(&source).as_os_str())?;
        let destination_name_c = c_name(&destination_name)?;
        let linked = before_link().and_then(|()| {
            // Follow only our procfs descriptor link, never the source pathname.
            status(unsafe {
                linkat(
                    AT_FDCWD,
                    pinned_name.as_ptr(),
                    destination_dir.0.as_raw_fd(),
                    destination_name_c.as_ptr(),
                    AT_SYMLINK_FOLLOW,
                )
            })
        });
        match linked {
            Ok(()) => {}
            Err(e)
                if matches!(
                    e.kind(),
                    io::ErrorKind::Unsupported | io::ErrorKind::PermissionDenied
                ) =>
            {
                let private = Staging::new(&destination_dir)?;
                let mut output = private.dir.new_file(OsStr::new("payload"))?;
                io::copy(&mut source, &mut output)?;
                copy_permissions(&output, &source.metadata()?)?;
                let payload = c_name(OsStr::new("payload"))?;
                // RENAME_NOREPLACE = 1. Both parents stay open through the move.
                status(unsafe {
                    renameat2(
                        private.dir.0.as_raw_fd(),
                        payload.as_ptr(),
                        destination_dir.0.as_raw_fd(),
                        destination_name_c.as_ptr(),
                        1,
                    )
                })?;
            }
            Err(e) => return Err(e),
        }
        // The caller owns the staging name exclusively. unlinkat cannot follow a
        // final symlink or be redirected by replacement of a parent pathname.
        // The destination already exists: a cleanup failure must not turn a
        // completed publication into an error (a retry could only collide).
        let _ = source_dir.unlink(&source_name, false);
        Ok(())
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        use std::io::Read;
        use std::os::unix::fs::symlink;

        #[test]
        fn pinned_source_survives_replacement_with_symlink_or_fifo() {
            unsafe extern "C" {
                fn mkfifo(path: *const c_char, mode: u32) -> c_int;
            }
            let root = tempfile::tempdir().unwrap();
            let outside = tempfile::tempdir().unwrap();
            fs::write(outside.path().join("secret"), b"outside").unwrap();
            let dir = Dir::open(root.path(), false).unwrap();
            for fifo in [false, true] {
                let path = root.path().join("source");
                fs::write(&path, b"original").unwrap();
                let mut file = dir
                    .node_with(OsStr::new("source"), || {
                        fs::remove_file(&path).unwrap();
                        if fifo {
                            let name = c_name(path.as_os_str()).unwrap();
                            assert_eq!(unsafe { mkfifo(name.as_ptr(), 0o600) }, 0);
                        } else {
                            symlink(outside.path().join("secret"), &path).unwrap();
                        }
                    })
                    .unwrap();
                let mut bytes = Vec::new();
                file.read_to_end(&mut bytes).unwrap();
                assert_eq!(bytes, b"original");
                // A FIFO already present at open is rejected without opening it.
                assert!(dir.node(OsStr::new("source")).is_err());
                fs::remove_file(path).unwrap();
            }
        }

        #[test]
        fn pinned_directories_do_not_follow_replaced_parents() {
            let root = tempfile::tempdir().unwrap();
            let outside = tempfile::tempdir().unwrap();
            fs::create_dir(root.path().join("directory")).unwrap();
            fs::write(root.path().join("directory/source"), b"original").unwrap();
            fs::write(outside.path().join("source"), b"outside").unwrap();
            let dir = Dir::open(&root.path().join("directory"), false).unwrap();
            fs::rename(root.path().join("directory"), root.path().join("retained")).unwrap();
            symlink(outside.path(), root.path().join("directory")).unwrap();
            let mut bytes = String::new();
            dir.node(OsStr::new("source"))
                .unwrap()
                .read_to_string(&mut bytes)
                .unwrap();
            assert_eq!(bytes, "original");
            dir.new_file(OsStr::new("written"))
                .unwrap()
                .write_all(b"safe")
                .unwrap();
            dir.mkdir(OsStr::new("nested")).unwrap();
            assert!(!outside.path().join("written").exists());
            assert!(!outside.path().join("nested").exists());
            assert!(Dir::open(&root.path().join("directory"), true).is_err());
            assert!(root.path().join("retained/written").exists());
        }

        #[test]
        fn recursive_copy_uses_pinned_directory_after_path_replacement() {
            let root = tempfile::tempdir().unwrap();
            let outside = tempfile::tempdir().unwrap();
            fs::create_dir(root.path().join("source")).unwrap();
            fs::create_dir(root.path().join("destination")).unwrap();
            fs::write(root.path().join("source/authored"), b"original").unwrap();
            fs::write(outside.path().join("secret"), b"outside").unwrap();
            let dir = Dir::open(root.path(), false).unwrap();
            let source = dir
                .node_with(OsStr::new("source"), || {
                    fs::rename(root.path().join("source"), root.path().join("retained")).unwrap();
                    symlink(outside.path(), root.path().join("source")).unwrap();
                })
                .unwrap();
            let destination = Dir::open(&root.path().join("destination"), false).unwrap();
            copy_node(source, &destination, OsStr::new("copied"), None, true).unwrap();
            assert_eq!(
                fs::read(root.path().join("destination/copied/authored")).unwrap(),
                b"original"
            );
            assert!(!root.path().join("destination/copied/secret").exists());
            symlink(
                outside.path().join("secret"),
                root.path().join("destination/new"),
            )
            .unwrap();
            assert!(destination.new_file(OsStr::new("new")).is_err());
            assert_eq!(fs::read(outside.path().join("secret")).unwrap(), b"outside");
        }

        #[test]
        fn copy_refuses_scratch_already_inside_source_by_inode() {
            let root = tempfile::tempdir().unwrap();
            fs::write(root.path().join("authored"), b"original").unwrap();
            let scratch = root.path().join("scratch");
            fs::create_dir(&scratch).unwrap();
            assert!(copy_tree(root.path(), &scratch, &scratch, false).is_err());
            assert!(!scratch.join("scratch").exists());
            assert_eq!(fs::read(root.path().join("authored")).unwrap(), b"original");
        }

        #[test]
        fn slice_copy_refuses_scratch_inside_selected_source() {
            // The slice destination does not exist yet; the scratch root does.
            let root = tempfile::tempdir().unwrap();
            fs::create_dir(root.path().join("selected")).unwrap();
            fs::write(root.path().join("selected/authored"), b"original").unwrap();
            let scratch = root.path().join("selected/scratch");
            fs::create_dir(&scratch).unwrap();
            let destination = scratch.join("selected");
            assert!(
                copy_tree(&root.path().join("selected"), &destination, &scratch, true).is_err()
            );
            assert!(!destination.join("scratch/selected").exists());
            assert_eq!(
                fs::read(root.path().join("selected/authored")).unwrap(),
                b"original"
            );
        }

        #[test]
        fn rename_fallback_sweeps_dead_staging_dirs() {
            let root = tempfile::tempdir().unwrap();
            let outside = tempfile::tempdir().unwrap();
            fs::write(outside.path().join("keep"), b"outside").unwrap();
            let dead = crate::workspace::dead_pid();
            let live = std::process::id();
            let stale = root.path().join(format!(".gdkit-publish-{dead}-7"));
            fs::create_dir(&stale).unwrap();
            fs::write(stale.join("payload"), b"leftover").unwrap();
            symlink(outside.path(), stale.join("link")).unwrap();
            let kept = [
                format!(".gdkit-publish-{live}-7"),
                format!(".gdkit-publish-{dead}-x"),
                format!(".gdkit-publish-{dead}"),
                format!("gdkit-publish-{dead}-7"),
            ];
            for name in &kept {
                fs::create_dir(root.path().join(name)).unwrap();
            }
            let link = format!(".gdkit-publish-{dead}-8");
            symlink(outside.path(), root.path().join(&link)).unwrap();
            let staged = root.path().join("staged");
            fs::write(&staged, b"payload").unwrap();
            publish(&staged, &root.path().join("published"), || {
                Err(io::ErrorKind::Unsupported.into())
            })
            .unwrap();
            assert!(!stale.exists());
            for name in kept.iter().chain([&link]) {
                assert!(
                    fs::symlink_metadata(root.path().join(name)).is_ok(),
                    "{name}"
                );
            }
            assert_eq!(fs::read(outside.path().join("keep")).unwrap(), b"outside");
            assert_eq!(fs::read(root.path().join("published")).unwrap(), b"payload");
        }

        #[test]
        fn publication_pins_both_parents_and_source_before_link_or_fallback() {
            for fallback in [false, true] {
                let root = tempfile::tempdir().unwrap();
                let outside = tempfile::tempdir().unwrap();
                fs::create_dir(root.path().join("work")).unwrap();
                let staged = root.path().join("work/staged");
                let destination = root.path().join("work/published");
                fs::write(&staged, b"original").unwrap();
                fs::write(outside.path().join("secret"), b"outside").unwrap();
                publish(&staged, &destination, || {
                    fs::rename(root.path().join("work"), root.path().join("retained")).unwrap();
                    symlink(outside.path(), root.path().join("work")).unwrap();
                    fs::rename(
                        root.path().join("retained/staged"),
                        root.path().join("retained/saved"),
                    )
                    .unwrap();
                    symlink(
                        outside.path().join("secret"),
                        root.path().join("retained/staged"),
                    )
                    .unwrap();
                    if fallback {
                        Err(io::ErrorKind::Unsupported.into())
                    } else {
                        Ok(())
                    }
                })
                .unwrap();
                assert_eq!(
                    fs::read(root.path().join("retained/published")).unwrap(),
                    b"original"
                );
                assert!(
                    !fs::symlink_metadata(root.path().join("retained/published"))
                        .unwrap()
                        .file_type()
                        .is_symlink()
                );
                assert!(!outside.path().join("published").exists());
                assert_eq!(fs::read(outside.path().join("secret")).unwrap(), b"outside");
                assert_eq!(
                    fs::read_dir(root.path().join("retained")).unwrap().count(),
                    2
                );
            }
        }
    }
}

#[cfg(windows)]
fn rename_new(from: &Path, to: &Path) -> io::Result<()> {
    use std::os::windows::ffi::OsStrExt;
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn MoveFileExW(from: *const u16, to: *const u16, flags: u32) -> i32;
    }
    let wide = |path: &Path| -> io::Result<Vec<u16>> {
        let mut value: Vec<_> = path.as_os_str().encode_wide().collect();
        if value.contains(&0) {
            return Err(io::Error::new(io::ErrorKind::InvalidInput, "NUL in path"));
        }
        value.push(0);
        Ok(value)
    };
    let from = wide(from)?;
    let to = wide(to)?;
    // No REPLACE_EXISTING or COPY_ALLOWED: same-filesystem atomic move only.
    if unsafe { MoveFileExW(from.as_ptr(), to.as_ptr(), 0) } != 0 {
        Ok(())
    } else {
        Err(io::Error::last_os_error())
    }
}

#[cfg(target_vendor = "apple")]
fn rename_new(from: &Path, to: &Path) -> io::Result<()> {
    use std::ffi::CString;
    use std::os::unix::ffi::OsStrExt;
    unsafe extern "C" {
        fn renamex_np(
            from: *const std::ffi::c_char,
            to: *const std::ffi::c_char,
            flags: u32,
        ) -> i32;
    }
    let from = CString::new(from.as_os_str().as_bytes())?;
    let to = CString::new(to.as_os_str().as_bytes())?;
    // RENAME_EXCL = 4: atomically refuse any existing destination.
    if unsafe { renamex_np(from.as_ptr(), to.as_ptr(), 4) } == 0 {
        Ok(())
    } else {
        Err(io::Error::last_os_error())
    }
}

#[cfg(not(any(
    all(target_os = "linux", any(target_arch = "x86", target_arch = "x86_64")),
    target_vendor = "apple",
    windows
)))]
fn rename_new(_from: &Path, _to: &Path) -> io::Result<()> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "atomic no-replace rename unavailable",
    ))
}

/// The pid of a child that has exited and been reaped.
#[cfg(all(test, unix))]
fn dead_pid() -> u32 {
    let mut child = std::process::Command::new("sh")
        .args(["-c", "exit 0"])
        .spawn()
        .unwrap();
    let pid = child.id();
    child.wait().unwrap();
    assert!(liveness::definitely_dead(pid));
    pid
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(unix)]
    #[test]
    fn allocation_sweeps_only_dead_owned_scratch_dirs() {
        use std::os::unix::fs::symlink;
        let temp = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        fs::write(outside.path().join("keep"), b"outside").unwrap();
        let dead = dead_pid();
        let name = |pid: u32, n: u64| format!("gdkit-{:020}-{pid:010}-{n:020}", 1);
        let stale = temp.path().join(name(dead, 1));
        fs::create_dir_all(stale.join(".godot")).unwrap();
        fs::write(stale.join("project.godot"), b"config_version=5\n").unwrap();
        symlink(outside.path(), stale.join("link")).unwrap();
        let kept = [
            name(std::process::id(), 2),
            name(1, 3),
            format!("gdkit-{dead}"),
            format!("{}-x", name(dead, 4)),
            format!("gdkit-{:019}-{dead:010}-{:020}", 1, 5),
            format!("x{}", name(dead, 6)),
        ];
        for kept in &kept {
            fs::create_dir(temp.path().join(kept)).unwrap();
        }
        let link = name(dead, 7);
        symlink(outside.path(), temp.path().join(&link)).unwrap();
        let file = name(dead, 8);
        fs::write(temp.path().join(&file), b"not a directory").unwrap();

        let copy = IsolatedCopy::allocate_in(temp.path(), None).unwrap();
        assert!(!stale.exists());
        for name in kept.iter().chain([&link, &file]) {
            assert!(
                fs::symlink_metadata(temp.path().join(name)).is_ok(),
                "{name}"
            );
        }
        assert!(copy.path().is_dir());
        assert_eq!(fs::read(outside.path().join("keep")).unwrap(), b"outside");
    }

    #[test]
    fn leftover_names_parse_only_in_their_exact_shape() {
        let pid = |value: u32| format!("{value:010}");
        let scratch = format!("gdkit-{:020}-{}-{:020}", 7, pid(42), 3);
        assert_eq!(scratch_pid(&scratch), Some(42));
        for bad in [
            format!("{scratch}-1"),
            format!("gdkit-{:020}-{}", 7, pid(42)),
            format!("gdkit-{:020}-{:09}-{:020}", 7, 42, 3),
            scratch.replace("-00", "-+0"),
            "gdkit-".to_owned(),
        ] {
            assert_eq!(scratch_pid(&bad), None, "{bad}");
        }
        assert_eq!(staging_pid(".gdkit-publish-42-0"), Some(42));
        for bad in [
            ".gdkit-publish-42",
            ".gdkit-publish--0",
            ".gdkit-publish-42-",
            ".gdkit-publish-42-0-1",
            ".gdkit-publish-+42-0",
            "gdkit-publish-42-0",
        ] {
            assert_eq!(staging_pid(bad), None, "{bad}");
        }
    }

    #[test]
    fn scratch_inside_project_is_rejected_before_allocation() {
        let root = tempfile::tempdir().unwrap();
        fs::write(root.path().join("project.godot"), b"config_version=5\n").unwrap();
        fs::create_dir(root.path().join("selected")).unwrap();
        fs::create_dir(root.path().join("selected/tmp")).unwrap();
        let project = Project::open(root.path()).unwrap();
        for temp in [root.path().to_path_buf(), root.path().join("selected/tmp")] {
            let before = fs::read_dir(&temp).unwrap().count();
            assert!(IsolatedCopy::full_in(&project, &temp).is_err());
            assert!(IsolatedCopy::slice_in(&project, &["selected".into()], &temp).is_err());
            assert_eq!(fs::read_dir(&temp).unwrap().count(), before);
        }
        #[cfg(unix)]
        {
            let outside = tempfile::tempdir().unwrap();
            let alias = outside.path().join("alias");
            std::os::unix::fs::symlink(root.path().join("selected/tmp"), &alias).unwrap();
            assert!(IsolatedCopy::full_in(&project, &alias).is_err());
            assert!(IsolatedCopy::slice_in(&project, &["selected".into()], &alias).is_err());
            assert_eq!(
                fs::read_dir(root.path().join("selected/tmp"))
                    .unwrap()
                    .count(),
                0
            );
        }
    }

    #[test]
    fn failed_copy_cleans_partial_scratch_without_touching_source() {
        let source = tempfile::tempdir().unwrap();
        fs::write(source.path().join("a-good"), b"authored").unwrap();
        let mut allocated = PathBuf::new();
        let result = (|| -> Result<IsolatedCopy> {
            let copy = IsolatedCopy::allocate()?;
            allocated = copy.path().to_path_buf();
            copy_tree(source.path(), copy.path(), copy.path(), false)?;
            assert!(copy.path().join("a-good").exists());
            copy_tree(
                &source.path().join("missing"),
                &copy.path().join("missing"),
                copy.path(),
                true,
            )?;
            Ok(copy)
        })();
        assert!(result.is_err());
        assert!(!allocated.exists());
        assert_eq!(fs::read(source.path().join("a-good")).unwrap(), b"authored");
    }

    #[test]
    fn publication_does_not_fall_back_for_unrelated_link_failures() {
        let dir = tempfile::tempdir().unwrap();
        let staged = dir.path().join("staged");
        let destination = dir.path().join("destination");
        fs::write(&staged, b"payload").unwrap();
        for kind in [
            io::ErrorKind::AlreadyExists,
            io::ErrorKind::NotFound,
            io::ErrorKind::Other,
        ] {
            assert!(publish_with(&staged, &destination, || Err(kind.into())).is_err());
            assert!(!destination.exists());
            assert_eq!(fs::read(&staged).unwrap(), b"payload");
        }
    }

    /// Both link failures that mean "no hard links here" take the rename path.
    /// It must publish the full payload with the staged mode, consume the
    /// staging file, leave no private staging dir behind, and never replace.
    #[test]
    fn publish_new_file_falls_back_from_hard_link_to_rename_on_filesystems_without_links() {
        let listing = |dir: &Path| {
            let mut names: Vec<_> = fs::read_dir(dir)
                .unwrap()
                .map(|entry| entry.unwrap().file_name().into_string().unwrap())
                .collect();
            names.sort();
            names
        };
        for kind in [io::ErrorKind::Unsupported, io::ErrorKind::PermissionDenied] {
            let dir = tempfile::tempdir().unwrap();
            let staged = dir.path().join("staged");
            let destination = dir.path().join("destination");
            let payload = vec![7_u8; 256 * 1024];
            fs::write(&staged, &payload).unwrap();
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                fs::set_permissions(&staged, fs::Permissions::from_mode(0o640)).unwrap();
            }
            let mut linked = false;
            let result = publish_with(&staged, &destination, || {
                linked = true;
                Err(kind.into())
            });
            assert!(linked, "the injected link failure must be reached");
            if cfg!(any(
                all(
                    target_os = "linux",
                    any(target_arch = "x86", target_arch = "x86_64")
                ),
                target_vendor = "apple",
                windows
            )) {
                result.unwrap();
                assert_eq!(fs::read(&destination).unwrap(), payload);
                assert_eq!(listing(dir.path()), ["destination"]);
                #[cfg(unix)]
                {
                    use std::os::unix::fs::PermissionsExt;
                    let mode = fs::metadata(&destination).unwrap().permissions().mode();
                    assert_eq!(mode & 0o777, 0o640);
                }
                fs::write(&staged, b"replacement").unwrap();
                assert!(publish_with(&staged, &destination, || Err(kind.into())).is_err());
                assert_eq!(fs::read(&destination).unwrap(), payload);
                assert_eq!(fs::read(&staged).unwrap(), b"replacement");
                assert_eq!(listing(dir.path()), ["destination", "staged"]);
            } else {
                assert!(result.is_err());
                assert!(!destination.exists());
                assert!(staged.exists());
            }
        }
    }
}
