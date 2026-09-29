use crate::{
    harness::AgentId,
    ollama::ToolDefinition,
};
use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::Read,
    path::{Component, Path, PathBuf},
};

const MAX_LIST_RESULTS: usize = 500;
const MAX_SEARCH_RESULTS: usize = 200;
const MAX_READ_BYTES: usize = 64 * 1024;
const MAX_SEARCH_FILE_BYTES: usize = 256 * 1024;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MutationRecord {
    pub path: String,
    pub before_sha256: Option<String>,
    pub after_sha256: String,
}

#[derive(Debug)]
pub struct ProjectToolRuntime {
    root: PathBuf,
    journal: Vec<MutationRecord>,
}

impl ProjectToolRuntime {
    pub fn new(root: &Path) -> Result<Self> {
        let root = root
            .canonicalize()
            .with_context(|| format!("failed to canonicalize project root {}", root.display()))?;
        Ok(Self {
            root,
            journal: Vec::new(),
        })
    }

    pub fn tool_definitions(&self, agent: AgentId) -> Vec<ToolDefinition> {
        let mut tools = vec![list_tool(), read_tool(), search_tool()];
        if matches!(agent, AgentId::Coder | AgentId::InternalFix) {
            tools.push(write_tool());
        }
        tools
    }

    pub fn execute(&mut self, agent: AgentId, name: &str, arguments: &Value) -> Result<Value> {
        match name {
            "project_list" => {
                self.require_read(agent)?;
                let args: ListArgs = serde_json::from_value(arguments.clone())?;
                Ok(serde_json::to_value(self.list(args.path.as_deref())?)?)
            }
            "project_read" => {
                self.require_read(agent)?;
                let args: ReadArgs = serde_json::from_value(arguments.clone())?;
                Ok(serde_json::to_value(self.read(&args.path)?)?)
            }
            "project_search" => {
                self.require_read(agent)?;
                let args: SearchArgs = serde_json::from_value(arguments.clone())?;
                Ok(serde_json::to_value(
                    self.search(&args.query, args.path.as_deref())?,
                )?)
            }
            "project_write" => {
                self.require_write(agent)?;
                let args: WriteArgs = serde_json::from_value(arguments.clone())?;
                Ok(serde_json::to_value(self.write(args)?)?)
            }
            other => bail!("unknown project tool {other}"),
        }
    }

    pub fn journal(&self) -> &[MutationRecord] {
        &self.journal
    }

    pub fn clear_journal(&mut self) {
        self.journal.clear();
    }

    pub fn change_set_id(&self) -> Result<String> {
        let canonical = serde_json::to_vec(&self.journal)?;
        Ok(sha256_bytes(&canonical))
    }

    fn require_read(&self, agent: AgentId) -> Result<()> {
        if matches!(
            agent,
            AgentId::General
                | AgentId::Planner
                | AgentId::JobBuilder
                | AgentId::Reviewer
                | AgentId::Coder
                | AgentId::Tester
                | AgentId::LocalCr
                | AgentId::InternalFix
        ) {
            Ok(())
        } else {
            bail!("{} is not allowed to read project files", agent.display_name())
        }
    }

    fn require_write(&self, agent: AgentId) -> Result<()> {
        if matches!(agent, AgentId::Coder | AgentId::InternalFix) {
            Ok(())
        } else {
            bail!("{} is read-only for project files", agent.display_name())
        }
    }

    fn list(&self, path: Option<&str>) -> Result<ListResult> {
        let relative = path.unwrap_or(".");
        let base = self.resolve_existing(relative, true)?;
        if !base.is_dir() {
            bail!("{relative} is not a directory");
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
                let path = entry.path();
                let rel = path
                    .strip_prefix(&self.root)
                    .context("listed path escaped project root")?;
                if should_skip(rel) {
                    continue;
                }
                let meta = fs::symlink_metadata(&path)?;
                if meta.file_type().is_symlink() {
                    continue;
                }
                if meta.is_dir() {
                    pending.push(path);
                    continue;
                }
                if !meta.is_file() {
                    continue;
                }
                if files.len() >= MAX_LIST_RESULTS {
                    truncated = true;
                    break;
                }
                files.push(rel.to_string_lossy().replace('\\', "/"));
            }
            if truncated {
                break;
            }
        }
        files.sort();
        Ok(ListResult { files, truncated })
    }

    fn read(&self, path: &str) -> Result<ReadResult> {
        let resolved = self.resolve_existing(path, false)?;
        let meta = fs::metadata(&resolved)?;
        if !meta.is_file() {
            bail!("{path} is not a regular file");
        }

        let mut file = fs::File::open(&resolved)?;
        let mut bytes = Vec::new();
        file.by_ref()
            .take((MAX_READ_BYTES + 1) as u64)
            .read_to_end(&mut bytes)?;
        let truncated = bytes.len() > MAX_READ_BYTES;
        if truncated {
            bytes.truncate(MAX_READ_BYTES);
        }
        let content = String::from_utf8(bytes).context("project_read supports UTF-8 text files only")?;
        let full_bytes = fs::read(&resolved)?;
        let sha256 = sha256_bytes(&full_bytes);
        Ok(ReadResult {
            path: normalize_display(path)?,
            content,
            sha256,
            truncated,
        })
    }

    fn search(&self, query: &str, path: Option<&str>) -> Result<SearchResult> {
        if query.is_empty() {
            bail!("search query cannot be empty");
        }
        let base = self.resolve_existing(path.unwrap_or("."), true)?;
        if !base.is_dir() {
            bail!("search path is not a directory");
        }

        let mut pending = vec![base];
        let mut matches = Vec::new();
        let mut truncated = false;
        while let Some(dir) = pending.pop() {
            let mut entries = fs::read_dir(&dir)?
                .filter_map(|entry| entry.ok())
                .collect::<Vec<_>>();
            entries.sort_by_key(|entry| entry.file_name());

            for entry in entries {
                let item = entry.path();
                let rel = item
                    .strip_prefix(&self.root)
                    .context("search path escaped project root")?;
                if should_skip(rel) {
                    continue;
                }
                let meta = fs::symlink_metadata(&item)?;
                if meta.file_type().is_symlink() {
                    continue;
                }
                if meta.is_dir() {
                    pending.push(item);
                    continue;
                }
                if !meta.is_file() || meta.len() as usize > MAX_SEARCH_FILE_BYTES {
                    continue;
                }
                let Ok(text) = fs::read_to_string(&item) else {
                    continue;
                };
                for (index, line) in text.lines().enumerate() {
                    if line.contains(query) {
                        if matches.len() >= MAX_SEARCH_RESULTS {
                            truncated = true;
                            break;
                        }
                        matches.push(SearchMatch {
                            path: rel.to_string_lossy().replace('\\', "/"),
                            line: index + 1,
                            text: line.to_owned(),
                        });
                    }
                }
                if truncated {
                    break;
                }
            }
            if truncated {
                break;
            }
        }
        Ok(SearchResult { matches, truncated })
    }

    fn write(&mut self, args: WriteArgs) -> Result<WriteResult> {
        let relative = normalize_relative(&args.path)?;
        let display = relative.to_string_lossy().replace('\\', "/");
        let target = self.root.join(&relative);
        let parent = target.parent().context("write target has no parent")?;
        self.assert_existing_components_inside(parent)?;

        if args.create_only {
            if target.exists() || fs::symlink_metadata(&target).is_ok() {
                bail!("create-only target already exists: {display}");
            }
            if args.expected_sha256.is_some() {
                bail!("create-only write must not provide expected_sha256");
            }
            fs::write(&target, args.content.as_bytes())?;
            let after = sha256_bytes(args.content.as_bytes());
            self.journal.push(MutationRecord {
                path: display.clone(),
                before_sha256: None,
                after_sha256: after.clone(),
            });
            return Ok(WriteResult {
                path: display,
                before_sha256: None,
                after_sha256: after,
            });
        }

        let resolved = self.resolve_existing(&args.path, false)?;
        let meta = fs::metadata(&resolved)?;
        if !meta.is_file() {
            bail!("write target is not a regular file");
        }
        let before_bytes = fs::read(&resolved)?;
        let before = sha256_bytes(&before_bytes);
        let expected = args
            .expected_sha256
            .as_deref()
            .context("editing an existing file requires expected_sha256 from project_read")?;
        if expected != before {
            bail!("stale write for {display}: expected {expected}, current {before}");
        }

        fs::write(&resolved, args.content.as_bytes())?;
        let after = sha256_bytes(args.content.as_bytes());
        self.journal.push(MutationRecord {
            path: display.clone(),
            before_sha256: Some(before.clone()),
            after_sha256: after.clone(),
        });
        Ok(WriteResult {
            path: display,
            before_sha256: Some(before),
            after_sha256: after,
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
                .with_context(|| format!("project path does not exist: {raw}"))?;
            if meta.file_type().is_symlink() {
                bail!("symlink paths are not allowed: {raw}");
            }
        }
        let canonical = cursor.canonicalize()?;
        if !canonical.starts_with(&self.root) {
            bail!("project path escaped root: {raw}");
        }
        if !allow_dir && canonical.is_dir() {
            bail!("{raw} is a directory");
        }
        Ok(canonical)
    }

    fn assert_existing_components_inside(&self, path: &Path) -> Result<()> {
        let relative = path
            .strip_prefix(&self.root)
            .context("write parent escaped project root")?;
        let mut cursor = self.root.clone();
        for component in relative.components() {
            let Component::Normal(part) = component else {
                continue;
            };
            cursor.push(part);
            let meta = fs::symlink_metadata(&cursor)
                .with_context(|| format!("write parent does not exist: {}", cursor.display()))?;
            if meta.file_type().is_symlink() {
                bail!("write through symlink is not allowed: {}", cursor.display());
            }
            if !meta.is_dir() {
                bail!("write parent component is not a directory: {}", cursor.display());
            }
        }
        let canonical = path.canonicalize()?;
        if !canonical.starts_with(&self.root) {
            bail!("write parent escaped project root");
        }
        Ok(())
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
struct SearchArgs {
    query: String,
    #[serde(default)]
    path: Option<String>,
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

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ListResult {
    pub files: Vec<String>,
    pub truncated: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ReadResult {
    pub path: String,
    pub content: String,
    pub sha256: String,
    pub truncated: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SearchResult {
    pub matches: Vec<SearchMatch>,
    pub truncated: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SearchMatch {
    pub path: String,
    pub line: usize,
    pub text: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct WriteResult {
    pub path: String,
    pub before_sha256: Option<String>,
    pub after_sha256: String,
}

fn normalize_relative(raw: &str) -> Result<PathBuf> {
    let path = Path::new(raw);
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

fn should_skip(relative: &Path) -> bool {
    relative.components().any(|component| {
        matches!(
            component.as_os_str().to_str(),
            Some(".git" | "target" | "node_modules" | ".gsa-local")
        )
    })
}

fn sha256_bytes(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    let mut output = String::with_capacity(digest.len() * 2);
    for byte in digest {
        use std::fmt::Write;
        let _ = write!(&mut output, "{byte:02x}");
    }
    output
}

fn list_tool() -> ToolDefinition {
    ToolDefinition::function(
        "project_list",
        "List bounded regular files inside the current project. Symlinks and common generated directories are skipped.",
        json!({
            "type":"object",
            "properties":{"path":{"type":"string","description":"Relative project directory; default ."}}
        }),
    )
}

fn read_tool() -> ToolDefinition {
    ToolDefinition::function(
        "project_read",
        "Read a bounded UTF-8 project file and return its SHA-256 for read-before-write CAS.",
        json!({
            "type":"object",
            "required":["path"],
            "properties":{"path":{"type":"string"}}
        }),
    )
}

fn search_tool() -> ToolDefinition {
    ToolDefinition::function(
        "project_search",
        "Search literal text inside bounded UTF-8 project files.",
        json!({
            "type":"object",
            "required":["query"],
            "properties":{
                "query":{"type":"string"},
                "path":{"type":"string","description":"Optional relative directory; default ."}
            }
        }),
    )
}

fn write_tool() -> ToolDefinition {
    ToolDefinition::function(
        "project_write",
        "Write one UTF-8 project file. Existing files require expected_sha256 from project_read. New files require create_only=true.",
        json!({
            "type":"object",
            "required":["path","content"],
            "properties":{
                "path":{"type":"string"},
                "content":{"type":"string"},
                "expected_sha256":{"type":"string"},
                "create_only":{"type":"boolean","default":false}
            }
        }),
    )
}
