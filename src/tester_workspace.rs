use crate::{harness::AgentId, ollama::ToolDefinition};
use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::{Read, Write},
    path::{Component, Path, PathBuf},
};

const MAX_LIST_RESULTS: usize = 500;
const MAX_READ_BYTES: usize = 64 * 1024;
const MAX_WRITE_BYTES: usize = MAX_READ_BYTES;
const WORKSPACE_DIRS: &[&str] = &["plan", "tests", "fixtures", "artifacts", "reports"];

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TesterArtifactRef {
    pub graph_version: i64,
    pub checkpoint_id: String,
    pub attempt_id: String,
    pub path: String,
    pub sha256: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TesterWorkspaceListResult {
    pub files: Vec<String>,
    pub truncated: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TesterWorkspaceReadResult {
    pub artifact: TesterArtifactRef,
    pub content: String,
    pub truncated: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TesterWorkspaceWriteResult {
    pub artifact: TesterArtifactRef,
    pub before_sha256: Option<String>,
}

#[derive(Debug)]
pub struct TesterWorkspaceRuntime {
    project_root: PathBuf,
    root: PathBuf,
    graph_version: i64,
    checkpoint_id: String,
    attempt_id: String,
}

impl TesterWorkspaceRuntime {
    pub fn new(
        project_root: &Path,
        graph_version: i64,
        checkpoint_id: &str,
        attempt_id: &str,
    ) -> Result<Self> {
        if graph_version <= 0 {
            bail!("Tester workspace graph_version must be positive");
        }
        validate_segment("checkpoint_id", checkpoint_id)?;
        validate_segment("attempt_id", attempt_id)?;

        let project_root = project_root.canonicalize().with_context(|| {
            format!(
                "failed to canonicalize project root {}",
                project_root.display()
            )
        })?;
        let relative = PathBuf::from(".gsa")
            .join("tester")
            .join(graph_version.to_string())
            .join(checkpoint_id)
            .join(attempt_id);
        let root = ensure_directory_chain(&project_root, &relative)?;

        for name in WORKSPACE_DIRS {
            ensure_directory_chain(&root, Path::new(name))?;
        }

        Ok(Self {
            project_root,
            root,
            graph_version,
            checkpoint_id: checkpoint_id.to_owned(),
            attempt_id: attempt_id.to_owned(),
        })
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn tool_definitions(&self, agent: AgentId) -> Vec<ToolDefinition> {
        if agent != AgentId::Tester {
            return vec![];
        }
        vec![
            tester_workspace_list_tool(),
            tester_workspace_read_tool(),
            tester_workspace_write_tool(),
        ]
    }

    pub fn execute(&self, agent: AgentId, name: &str, arguments: &Value) -> Result<Value> {
        if agent != AgentId::Tester {
            bail!(
                "{} is not allowed to use Tester workspace tools",
                agent.display_name()
            );
        }
        match name {
            "tester_workspace_list" => {
                let args: ListArgs = serde_json::from_value(arguments.clone())?;
                Ok(serde_json::to_value(self.list(args.path.as_deref())?)?)
            }
            "tester_workspace_read" => {
                let args: ReadArgs = serde_json::from_value(arguments.clone())?;
                Ok(serde_json::to_value(self.read(&args.path)?)?)
            }
            "tester_workspace_write" => {
                let args: WriteArgs = serde_json::from_value(arguments.clone())?;
                Ok(serde_json::to_value(self.write(args)?)?)
            }
            other => bail!("unknown Tester workspace tool {other}"),
        }
    }

    pub fn artifact_ref(&self, path: &str) -> Result<TesterArtifactRef> {
        let resolved = self.resolve_existing(path, false)?;
        let meta = fs::metadata(&resolved)?;
        if !meta.is_file() {
            bail!("Tester artifact is not a regular file");
        }
        reject_multi_link_file(&meta)?;
        let sha256 = sha256_file(&resolved)?;
        Ok(TesterArtifactRef {
            graph_version: self.graph_version,
            checkpoint_id: self.checkpoint_id.clone(),
            attempt_id: self.attempt_id.clone(),
            path: normalize_display(path)?,
            sha256,
        })
    }

    fn list(&self, path: Option<&str>) -> Result<TesterWorkspaceListResult> {
        let base = self.resolve_existing(path.unwrap_or("."), true)?;
        if !base.is_dir() {
            bail!("Tester workspace list path is not a directory");
        }

        let mut pending = vec![base];
        let mut files = Vec::new();
        let mut truncated = false;
        while let Some(dir) = pending.pop() {
            let mut entries = fs::read_dir(&dir)?
                .filter_map(|entry| entry.ok())
                .collect::<Vec<_>>();
            entries.sort_by_key(|entry| entry.file_name());

            for entry in entries {
                let item = entry.path();
                let meta = fs::symlink_metadata(&item)?;
                if meta.file_type().is_symlink() {
                    continue;
                }
                if meta.is_dir() {
                    pending.push(item);
                    continue;
                }
                if !meta.is_file() {
                    continue;
                }
                if files.len() >= MAX_LIST_RESULTS {
                    truncated = true;
                    break;
                }
                let rel = item
                    .strip_prefix(&self.root)
                    .context("Tester workspace list escaped attempt root")?;
                files.push(rel.to_string_lossy().replace('\\', "/"));
            }
            if truncated {
                break;
            }
        }
        files.sort();
        Ok(TesterWorkspaceListResult { files, truncated })
    }

    fn read(&self, path: &str) -> Result<TesterWorkspaceReadResult> {
        let resolved = self.resolve_existing(path, false)?;
        let meta = fs::metadata(&resolved)?;
        if !meta.is_file() {
            bail!("Tester workspace read target is not a regular file");
        }
        reject_multi_link_file(&meta)?;

        let mut file = fs::File::open(&resolved)?;
        let mut bytes = Vec::new();
        Read::by_ref(&mut file)
            .take((MAX_READ_BYTES + 1) as u64)
            .read_to_end(&mut bytes)?;
        if bytes.len() > MAX_READ_BYTES {
            bail!(
                "tester_workspace_read refuses files larger than {} bytes",
                MAX_READ_BYTES
            );
        }
        let content =
            String::from_utf8(bytes).context("Tester workspace tools support UTF-8 text only")?;
        Ok(TesterWorkspaceReadResult {
            artifact: self.artifact_ref(path)?,
            content,
            truncated: false,
        })
    }

    fn write(&self, args: WriteArgs) -> Result<TesterWorkspaceWriteResult> {
        if args.content.len() > MAX_WRITE_BYTES {
            bail!(
                "tester_workspace_write content exceeds {} bytes",
                MAX_WRITE_BYTES
            );
        }

        let relative = normalize_relative(&args.path)?;
        if relative == Path::new(".") {
            bail!("Tester workspace write requires a file path");
        }
        let display = relative.to_string_lossy().replace('\\', "/");
        let parent = relative.parent().unwrap_or_else(|| Path::new("."));
        ensure_directory_chain(&self.root, parent)?;
        let target = self.root.join(&relative);

        if args.create_only {
            if fs::symlink_metadata(&target).is_ok() {
                bail!("create-only Tester workspace target already exists: {display}");
            }
            if args.expected_sha256.is_some() {
                bail!("create-only Tester workspace write must not provide expected_sha256");
            }
            let mut file = fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&target)
                .with_context(|| {
                    format!("failed create-only Tester workspace write for {display}")
                })?;
            file.write_all(args.content.as_bytes())?;
            return Ok(TesterWorkspaceWriteResult {
                artifact: self.artifact_ref(&display)?,
                before_sha256: None,
            });
        }

        let resolved = self.resolve_existing(&args.path, false)?;
        let meta = fs::metadata(&resolved)?;
        if !meta.is_file() {
            bail!("Tester workspace write target is not a regular file");
        }
        reject_multi_link_file(&meta)?;
        if meta.len() as usize > MAX_READ_BYTES {
            bail!(
                "tester_workspace_write refuses existing files larger than {} bytes",
                MAX_READ_BYTES
            );
        }

        let before_content = fs::read_to_string(&resolved)
            .context("Tester workspace tools support UTF-8 text only")?;
        let before_sha256 = sha256_bytes(before_content.as_bytes());
        let expected = args.expected_sha256.as_deref().context(
            "editing an existing Tester workspace file requires expected_sha256 from tester_workspace_read",
        )?;
        if expected != before_sha256 {
            bail!(
                "stale Tester workspace write for {display}: expected {expected}, current {before_sha256}"
            );
        }
        let after_sha256 = sha256_bytes(args.content.as_bytes());
        if after_sha256 == before_sha256 {
            bail!("tester_workspace_write refuses a no-op edit for {display}");
        }

        fs::write(&resolved, args.content.as_bytes())?;
        Ok(TesterWorkspaceWriteResult {
            artifact: self.artifact_ref(&display)?,
            before_sha256: Some(before_sha256),
        })
    }

    fn resolve_existing(&self, raw: &str, allow_dir: bool) -> Result<PathBuf> {
        let relative = normalize_relative(raw)?;
        let mut cursor = self.root.clone();
        for component in relative.components() {
            let Component::Normal(part) = component else {
                continue;
            };
            cursor.push(part);
            let meta = fs::symlink_metadata(&cursor)
                .with_context(|| format!("Tester workspace path does not exist: {raw}"))?;
            if meta.file_type().is_symlink() {
                bail!("symlink paths are not allowed in Tester workspace: {raw}");
            }
        }

        let canonical = cursor.canonicalize()?;
        if !canonical.starts_with(&self.root) {
            bail!("Tester workspace path escaped attempt root: {raw}");
        }
        if !canonical.starts_with(&self.project_root) {
            bail!("Tester workspace path escaped project root: {raw}");
        }
        if !allow_dir && canonical.is_dir() {
            bail!("{raw} is a directory");
        }
        Ok(canonical)
    }
}

#[derive(Debug, Deserialize)]
struct ListArgs {
    #[serde(default)]
    path: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ReadArgs {
    path: String,
}

#[derive(Debug, Deserialize)]
struct WriteArgs {
    path: String,
    content: String,
    #[serde(default)]
    expected_sha256: Option<String>,
    #[serde(default)]
    create_only: bool,
}

fn validate_segment(name: &str, value: &str) -> Result<()> {
    if value.is_empty() || value == "." || value == ".." || value.len() > 128 {
        bail!("invalid Tester workspace {name}");
    }
    if !value
        .bytes()
        .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
    {
        bail!("Tester workspace {name} contains unsafe characters");
    }
    Ok(())
}

fn ensure_directory_chain(root: &Path, relative: &Path) -> Result<PathBuf> {
    let relative = normalize_relative_path(relative)?;
    let mut cursor = root.to_path_buf();

    for component in relative.components() {
        let Component::Normal(part) = component else {
            continue;
        };
        cursor.push(part);
        match fs::symlink_metadata(&cursor) {
            Ok(meta) => {
                if meta.file_type().is_symlink() {
                    bail!(
                        "Tester workspace directory path contains symlink: {}",
                        cursor.display()
                    );
                }
                if !meta.is_dir() {
                    bail!(
                        "Tester workspace directory component is not a directory: {}",
                        cursor.display()
                    );
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                fs::create_dir(&cursor).with_context(|| {
                    format!(
                        "failed to create Tester workspace directory {}",
                        cursor.display()
                    )
                })?;
                let meta = fs::symlink_metadata(&cursor)?;
                if meta.file_type().is_symlink() || !meta.is_dir() {
                    bail!(
                        "unsafe Tester workspace directory created: {}",
                        cursor.display()
                    );
                }
            }
            Err(error) => return Err(error.into()),
        }
    }

    let canonical_root = root.canonicalize()?;
    let canonical = cursor.canonicalize()?;
    if !canonical.starts_with(&canonical_root) {
        bail!("Tester workspace directory escaped its allowed root");
    }
    Ok(canonical)
}

fn normalize_relative(raw: &str) -> Result<PathBuf> {
    normalize_relative_path(Path::new(raw))
}

fn normalize_relative_path(path: &Path) -> Result<PathBuf> {
    if path.is_absolute() {
        bail!("absolute paths are not allowed");
    }
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            Component::Normal(part) => normalized.push(part),
            Component::CurDir => {}
            Component::ParentDir => bail!("parent traversal is not allowed"),
            Component::RootDir | Component::Prefix(_) => bail!("absolute paths are not allowed"),
        }
    }
    if normalized.as_os_str().is_empty() {
        Ok(PathBuf::from("."))
    } else {
        Ok(normalized)
    }
}

fn normalize_display(raw: &str) -> Result<String> {
    Ok(normalize_relative(raw)?
        .to_string_lossy()
        .replace('\\', "/"))
}

#[cfg(unix)]
fn reject_multi_link_file(metadata: &fs::Metadata) -> Result<()> {
    use std::os::unix::fs::MetadataExt;
    if metadata.nlink() > 1 {
        bail!("refusing Tester workspace access to a file with multiple hard links");
    }
    Ok(())
}

#[cfg(not(unix))]
fn reject_multi_link_file(_metadata: &fs::Metadata) -> Result<()> {
    Ok(())
}

fn sha256_file(path: &Path) -> Result<String> {
    let mut file = fs::File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buffer = [0u8; 8192];
    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(hex_digest(hasher.finalize()))
}

fn sha256_bytes(bytes: &[u8]) -> String {
    hex_digest(Sha256::digest(bytes))
}

fn hex_digest(digest: impl AsRef<[u8]>) -> String {
    let digest = digest.as_ref();
    let mut output = String::with_capacity(digest.len() * 2);
    for byte in digest {
        use std::fmt::Write;
        let _ = write!(&mut output, "{byte:02x}");
    }
    output
}

fn tester_workspace_list_tool() -> ToolDefinition {
    ToolDefinition::function(
        "tester_workspace_list",
        "List bounded regular files inside the exact Tester checkpoint attempt workspace.",
        json!({
            "type": "object",
            "properties": {
                "path": {"type": "string", "description": "Relative Tester workspace directory; default ."}
            }
        }),
    )
}

fn tester_workspace_read_tool() -> ToolDefinition {
    ToolDefinition::function(
        "tester_workspace_read",
        "Read one bounded UTF-8 Tester-owned file and return its stable artifact identity/hash.",
        json!({
            "type": "object",
            "required": ["path"],
            "properties": {"path": {"type": "string"}}
        }),
    )
}

fn tester_workspace_write_tool() -> ToolDefinition {
    ToolDefinition::function(
        "tester_workspace_write",
        "Write one bounded UTF-8 file inside the exact Tester attempt workspace. Existing files require expected_sha256 from tester_workspace_read.",
        json!({
            "type": "object",
            "required": ["path", "content"],
            "properties": {
                "path": {"type": "string"},
                "content": {"type": "string"},
                "expected_sha256": {"type": "string"},
                "create_only": {"type": "boolean", "default": false}
            }
        }),
    )
}
