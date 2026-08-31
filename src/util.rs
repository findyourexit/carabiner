use anyhow::{anyhow, Context, Result};
use serde_json::{Map, Value};
use std::fs;
use std::io::Write;
use std::path::{Component, Path, PathBuf};
use walkdir::WalkDir;

/// Remove JSONC comments and trailing commas without touching string literals.
pub fn parse_jsonc(input: &str) -> Result<Value> {
    let mut without_comments = String::with_capacity(input.len());
    let mut chars = input.chars().peekable();
    let mut in_string = false;
    let mut escaped = false;
    while let Some(character) = chars.next() {
        if in_string {
            without_comments.push(character);
            if escaped {
                escaped = false;
            } else if character == '\\' {
                escaped = true;
            } else if character == '"' {
                in_string = false;
            }
            continue;
        }
        if character == '"' {
            in_string = true;
            without_comments.push(character);
            continue;
        }
        if character == '/' {
            match chars.peek().copied() {
                Some('/') => {
                    chars.next();
                    without_comments.push_str("  ");
                    for next in chars.by_ref() {
                        if next == '\n' || next == '\r' {
                            without_comments.push(next);
                            break;
                        }
                        without_comments.push(' ');
                    }
                    continue;
                }
                Some('*') => {
                    chars.next();
                    without_comments.push_str("  ");
                    while let Some(next) = chars.next() {
                        if next == '*' && chars.peek().copied() == Some('/') {
                            chars.next();
                            without_comments.push_str("  ");
                            break;
                        }
                        without_comments.push(if next == '\n' || next == '\r' {
                            next
                        } else {
                            ' '
                        });
                    }
                    continue;
                }
                _ => {}
            }
        }
        without_comments.push(character);
    }

    let mut result = String::with_capacity(without_comments.len());
    let chars = without_comments.chars().collect::<Vec<_>>();
    let mut in_string = false;
    let mut escaped = false;
    let mut index = 0;
    while index < chars.len() {
        let character = chars[index];
        if in_string {
            result.push(character);
            if escaped {
                escaped = false;
            } else if character == '\\' {
                escaped = true;
            } else if character == '"' {
                in_string = false;
            }
            index += 1;
            continue;
        }
        if character == '"' {
            in_string = true;
            result.push(character);
            index += 1;
            continue;
        }
        if character == ',' {
            let mut look = index + 1;
            while look < chars.len() && chars[look].is_whitespace() {
                look += 1;
            }
            if look < chars.len() && (chars[look] == '}' || chars[look] == ']') {
                index += 1;
                continue;
            }
        }
        result.push(character);
        index += 1;
    }

    serde_json::from_str(&result).context("failed to parse JSONC")
}

pub fn read_jsonc(path: &Path) -> Result<Value> {
    let content =
        fs::read_to_string(path).with_context(|| format!("failed to read {}", path.display()))?;
    parse_jsonc(&content).with_context(|| format!("failed to parse {}", path.display()))
}

pub fn json_pretty(value: &Value) -> Result<String> {
    Ok(format!("{}\n", serde_json::to_string_pretty(value)?))
}

pub fn ensure_parent(path: &Path) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create {}", parent.display()))?;
    }
    Ok(())
}

pub fn assert_no_symlink_path(root: &Path, target: &Path) -> Result<()> {
    if !target.starts_with(root) {
        return Err(anyhow!(
            "path {} is outside managed root {}",
            target.display(),
            root.display()
        ));
    }
    let mut current = target;
    loop {
        match fs::symlink_metadata(current) {
            Ok(metadata) if metadata.file_type().is_symlink() => {
                return Err(anyhow!(
                    "refusing to access path through symbolic link {}",
                    current.display()
                ));
            }
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => {
                return Err(error).with_context(|| {
                    format!("failed to inspect path component {}", current.display())
                });
            }
        }
        if current == root {
            break;
        }
        let Some(parent) = current.parent() else {
            break;
        };
        if !parent.starts_with(root) {
            break;
        }
        current = parent;
    }
    Ok(())
}

pub fn write_text_raw(path: &Path, content: &str, dry_run: bool) -> Result<bool> {
    if fs::symlink_metadata(path)
        .map(|metadata| metadata.file_type().is_symlink())
        .unwrap_or(false)
    {
        return Err(anyhow!(
            "refusing to write through symbolic link {}",
            path.display()
        ));
    }
    let existing = fs::read_to_string(path).ok();
    if existing.as_deref() == Some(content) {
        return Ok(false);
    }
    if !dry_run {
        ensure_parent(path)?;
        fs::write(path, content.as_bytes())
            .with_context(|| format!("failed to write {}", path.display()))?;
    }
    Ok(true)
}
pub fn write_text(path: &Path, content: &str, dry_run: bool) -> Result<bool> {
    if fs::symlink_metadata(path)
        .map(|metadata| metadata.file_type().is_symlink())
        .unwrap_or(false)
    {
        return Err(anyhow!(
            "refusing to write through symbolic link {}",
            path.display()
        ));
    }
    let normalized = add_trailing_newline(content);
    let existing = fs::read_to_string(path).ok();
    if existing.as_deref() == Some(normalized.as_str()) {
        return Ok(false);
    }
    if !dry_run {
        ensure_parent(path)?;
        fs::write(path, normalized.as_bytes())
            .with_context(|| format!("failed to write {}", path.display()))?;
    }
    Ok(true)
}

pub fn write_bytes(path: &Path, content: &[u8], dry_run: bool) -> Result<bool> {
    if fs::symlink_metadata(path)
        .map(|metadata| metadata.file_type().is_symlink())
        .unwrap_or(false)
    {
        return Err(anyhow!(
            "refusing to write through symbolic link {}",
            path.display()
        ));
    }
    let existing = fs::read(path).ok();
    if existing.as_deref() == Some(content) {
        return Ok(false);
    }
    if !dry_run {
        ensure_parent(path)?;
        let mut file = fs::File::create(path)
            .with_context(|| format!("failed to write {}", path.display()))?;
        file.write_all(content)
            .with_context(|| format!("failed to write {}", path.display()))?;
    }
    Ok(true)
}

pub fn add_trailing_newline(content: &str) -> String {
    let normalized = content.replace("\r\n", "\n").replace('\r', "\n");
    format!("{}\n", normalized.trim_end())
}

pub fn walk_files(root: &Path) -> Vec<PathBuf> {
    if !root.is_dir() {
        return Vec::new();
    }
    let mut files = WalkDir::new(root)
        .follow_links(false)
        .into_iter()
        .filter_map(Result::ok)
        .filter(|entry| entry.file_type().is_file())
        .map(|entry| entry.into_path())
        .collect::<Vec<_>>();
    files.sort();
    files
}

/// Discover source files through symbolic links. Deletion callers use
/// `walk_files` deliberately so managed-directory symlinks cannot escape the
/// output root.
pub fn walk_files_following_links(root: &Path) -> Vec<PathBuf> {
    if !root.is_dir() {
        return Vec::new();
    }
    let mut files = WalkDir::new(root)
        .follow_links(true)
        .into_iter()
        .filter_map(Result::ok)
        .filter(|entry| entry.file_type().is_file())
        .map(|entry| entry.into_path())
        .collect::<Vec<_>>();
    files.sort();
    files
}

pub fn direct_dirs(root: &Path) -> Vec<PathBuf> {
    if !root.is_dir() {
        return Vec::new();
    }
    let mut dirs = fs::read_dir(root)
        .into_iter()
        .flatten()
        .filter_map(Result::ok)
        .filter_map(|entry| {
            let path = entry.path();
            entry
                .file_type()
                .ok()
                .filter(|t| t.is_dir() || path.is_dir())
                .map(|_| path)
        })
        .collect::<Vec<_>>();
    dirs.sort();
    dirs
}

pub fn relative_slash(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
}

pub fn safe_relative_path(path: &str) -> Result<()> {
    if path.is_empty() {
        return Err(anyhow!("path must not be empty"));
    }
    if path.contains('\0') {
        return Err(anyhow!("path contains a NUL character"));
    }
    let candidate = Path::new(path);
    if candidate.is_absolute() {
        return Err(anyhow!("path must be relative: {path}"));
    }
    for component in candidate.components() {
        if matches!(
            component,
            Component::ParentDir | Component::RootDir | Component::Prefix(_)
        ) {
            return Err(anyhow!("path traversal detected: {path}"));
        }
    }
    Ok(())
}

pub fn safe_name(name: &str) -> Result<()> {
    if name.is_empty() || name == "." || name == ".." || name.ends_with('.') || name.ends_with(' ')
    {
        return Err(anyhow!("invalid name: {name}"));
    }
    if name.contains('/') || name.contains('\\') || name.contains('\0') {
        return Err(anyhow!("name must not contain path separators: {name}"));
    }
    Ok(())
}

pub fn has_control_chars(value: &str) -> bool {
    value.chars().any(|c| c == '\n' || c == '\r' || c == '\0')
}

fn repair_frontmatter_yaml(yaml: &str) -> String {
    yaml.lines()
        .map(|line| {
            let trimmed = line.trim_start();
            if trimmed != line || trimmed.starts_with('#') {
                return line.to_owned();
            }
            let Some((key, raw)) = line.split_once(':') else {
                return line.to_owned();
            };
            let value = raw.trim_start();
            if value.is_empty() || value.starts_with(['"', '\'', '|', '>', '[', '{', '&', '!', '#'])
            {
                return line.to_owned();
            }
            if (key == "globs" || key == "applyTo") && value.starts_with('*') {
                return format!(
                    "{}: {}",
                    key,
                    serde_json::to_string(value).unwrap_or_else(|_| format!("\"{value}\""))
                );
            }
            if value.contains(": ") {
                let without_comment = value
                    .split_once(" #")
                    .map(|(head, _)| head.trim_end())
                    .unwrap_or(value);
                return format!(
                    "{}: {}",
                    key,
                    serde_json::to_string(without_comment)
                        .unwrap_or_else(|_| format!("\"{without_comment}\""))
                );
            }
            line.to_owned()
        })
        .collect::<Vec<_>>()
        .join("\n")
}

pub fn parse_frontmatter(content: &str, path: &Path) -> Result<Frontmatter> {
    let content = content.strip_prefix('\u{feff}').unwrap_or(content);
    let mut lines = content.split_inclusive('\n');
    let first = lines.next().unwrap_or("");
    if first.trim_end_matches(['\r', '\n']).trim() != "---" {
        return Ok(Frontmatter {
            data: Map::new(),
            body: content.trim().to_owned(),
            has_frontmatter: false,
        });
    }
    let mut yaml = String::new();
    let mut body_start = None;
    let mut offset = first.len();
    for line in lines {
        let bare = line.trim_end_matches(['\r', '\n']);
        if bare.trim() == "---" {
            body_start = Some(offset + line.len());
            break;
        }
        yaml.push_str(line);
        offset += line.len();
    }
    let Some(body_start) = body_start else {
        return Err(anyhow!(
            "missing closing frontmatter delimiter in {}",
            path.display()
        ));
    };
    let yaml_value: serde_yaml::Value = if yaml.trim().is_empty() {
        serde_yaml::Value::Mapping(Default::default())
    } else {
        serde_yaml::from_str(&repair_frontmatter_yaml(&yaml))
            .with_context(|| format!("failed to parse frontmatter in {}", path.display()))?
    };
    let json_value = serde_json::to_value(yaml_value)
        .with_context(|| format!("invalid frontmatter in {}", path.display()))?;
    let data = match json_value {
        Value::Object(object) => object,
        Value::Null => Map::new(),
        _ => {
            return Err(anyhow!(
                "frontmatter in {} must be an object",
                path.display()
            ))
        }
    };
    Ok(Frontmatter {
        data,
        body: content[body_start..].trim().to_owned(),
        has_frontmatter: true,
    })
}

#[derive(Debug, Clone)]
pub struct Frontmatter {
    pub data: Map<String, Value>,
    pub body: String,
    pub has_frontmatter: bool,
}

fn remove_nullish(value: &Value) -> Option<Value> {
    match value {
        Value::Null => None,
        Value::Array(values) => Some(Value::Array(
            values.iter().filter_map(remove_nullish).collect(),
        )),
        Value::Object(object) => Some(Value::Object(
            object
                .iter()
                .filter_map(|(key, value)| remove_nullish(value).map(|value| (key.clone(), value)))
                .collect(),
        )),
        other => Some(other.clone()),
    }
}
fn flatten_strings(value: &Value) -> Option<Value> {
    match value {
        Value::Null => None,
        Value::String(text) => {
            let mut flattened = String::with_capacity(text.len());
            let mut in_newline_run = false;
            for character in text.chars() {
                if character == '\n' {
                    if !in_newline_run {
                        flattened.push(' ');
                    }
                    in_newline_run = true;
                } else {
                    flattened.push(character);
                    in_newline_run = false;
                }
            }
            Some(Value::String(flattened.trim().to_owned()))
        }
        Value::Array(values) => Some(Value::Array(
            values.iter().filter_map(flatten_strings).collect(),
        )),
        Value::Object(object) => Some(Value::Object(
            object
                .iter()
                .filter_map(|(key, value)| flatten_strings(value).map(|value| (key.clone(), value)))
                .collect(),
        )),
        other => Some(other.clone()),
    }
}

pub fn indent_yaml_sequences(yaml: &str) -> String {
    let mut output = Vec::new();
    let mut sequence_indents = Vec::new();
    for line in yaml.lines() {
        let trimmed = line.trim_start();
        let indent = line.len() - trimmed.len();
        if trimmed.is_empty() {
            output.push(line.to_owned());
            continue;
        }
        while sequence_indents
            .last()
            .is_some_and(|parent_indent| *parent_indent >= indent)
        {
            sequence_indents.pop();
        }
        let is_sequence_item = trimmed.starts_with("- ");
        let added_indent = 2 * (sequence_indents.len() + usize::from(is_sequence_item));
        output.push(format!("{}{}", " ".repeat(indent + added_indent), trimmed));
        if is_sequence_item {
            sequence_indents.push(indent);
        }
    }
    output.join("\n")
}

fn wrap_yaml_long_scalars(yaml: &str) -> String {
    let mut output = Vec::new();
    for line in yaml.lines() {
        let trimmed = line.trim_start();
        let indentation = line.len() - trimmed.len();
        let Some(separator) = trimmed.find(": ") else {
            output.push(line.to_owned());
            continue;
        };
        let value = &trimmed[separator + 2..];
        if line.chars().count() <= 80
            || value.is_empty()
            || value.starts_with(['"', '\'', '|', '>', '[', '{', '&', '*', '!'])
            || !value.chars().any(char::is_whitespace)
        {
            output.push(line.to_owned());
            continue;
        }
        output.push(format!(
            "{}{}: >-",
            &line[..indentation],
            &trimmed[..separator]
        ));
        let continuation_indent = " ".repeat(indentation + 2);
        let mut current = String::new();
        for word in value.split_whitespace() {
            let candidate = if current.is_empty() {
                word.to_owned()
            } else {
                format!("{current} {word}")
            };
            if !current.is_empty() && indentation + 2 + candidate.chars().count() > 80 {
                output.push(format!("{continuation_indent}{current}"));
                current = word.to_owned();
            } else {
                current = candidate;
            }
        }
        if !current.is_empty() {
            output.push(format!("{continuation_indent}{current}"));
        }
    }
    output.join("\n")
}

pub fn stringify_frontmatter(body: &str, data: &Map<String, Value>) -> Result<String> {
    let cleaned = remove_nullish(&Value::Object(data.clone()))
        .and_then(|value| value.as_object().cloned())
        .unwrap_or_default();
    let yaml_value = Value::Object(cleaned);
    let yaml = serde_yaml::to_string(&yaml_value).context("failed to serialize frontmatter")?;
    if yaml.trim() == "{}" {
        return Ok(format!("{}\n", body.trim()));
    }
    let yaml = format!(
        "{}\n",
        wrap_yaml_long_scalars(&indent_yaml_sequences(&yaml))
    );
    Ok(format!("---\n{}---\n{}\n", yaml, body.trim()))
}

pub fn object_value(value: &Value) -> Option<&Map<String, Value>> {
    match value {
        Value::Object(map) => Some(map),
        _ => None,
    }
}

pub fn object_value_mut(value: &mut Value) -> Option<&mut Map<String, Value>> {
    match value {
        Value::Object(map) => Some(map),
        _ => None,
    }
}
pub fn stringify_frontmatter_flat(body: &str, data: &Map<String, Value>) -> Result<String> {
    let cleaned = flatten_strings(&Value::Object(data.clone()))
        .and_then(|value| value.as_object().cloned())
        .unwrap_or_default();
    let yaml = serde_yaml::to_string(&Value::Object(cleaned))
        .context("failed to serialize frontmatter")?;
    if yaml.trim() == "{}" {
        return Ok(format!("{}\n", body.trim()));
    }
    let yaml = format!("{}\n", indent_yaml_sequences(&yaml));
    Ok(format!("---\n{}---\n{}\n", yaml, body.trim()))
}

pub fn map_without_keys(map: &Map<String, Value>, keys: &[&str]) -> Map<String, Value> {
    map.iter()
        .filter(|(key, _)| !keys.iter().any(|wanted| key == wanted))
        .map(|(key, value)| (key.clone(), value.clone()))
        .collect()
}

pub fn merge_maps(base: &Map<String, Value>, overlay: &Map<String, Value>) -> Map<String, Value> {
    let mut merged = base.clone();
    for (key, value) in overlay {
        if key != "__proto__" && key != "constructor" && key != "prototype" {
            merged.insert(key.clone(), value.clone());
        }
    }
    merged
}

pub fn get_string(map: &Map<String, Value>, key: &str) -> Option<String> {
    map.get(key).and_then(Value::as_str).map(ToOwned::to_owned)
}

pub fn get_bool(map: &Map<String, Value>, key: &str) -> Option<bool> {
    map.get(key).and_then(Value::as_bool)
}

pub fn get_string_array(map: &Map<String, Value>, key: &str) -> Option<Vec<String>> {
    map.get(key).and_then(|value| {
        value.as_array().map(|items| {
            items
                .iter()
                .filter_map(Value::as_str)
                .map(ToOwned::to_owned)
                .collect()
        })
    })
}

pub fn is_empty_payload(path: &Path, content: &str) -> bool {
    let trimmed = content.trim();
    if trimmed.is_empty() || trimmed == "{}" || trimmed == "[]" {
        return true;
    }
    if path.extension().and_then(|s| s.to_str()) == Some("json")
        || path.extension().and_then(|s| s.to_str()) == Some("jsonc")
    {
        if let Ok(value) = parse_jsonc(trimmed) {
            return value.as_object().map(|m| m.is_empty()).unwrap_or(false);
        }
    }
    false
}

pub fn home_dir() -> Result<PathBuf> {
    std::env::var_os("HOME_DIR")
        .or_else(|| std::env::var_os("HOME"))
        .map(PathBuf::from)
        .ok_or_else(|| anyhow!("HOME is not set"))
}
