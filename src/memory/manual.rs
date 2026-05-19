use super::*;

pub fn load_ingest_state(path: impl AsRef<Path>) -> Result<IngestState> {
    let path = path.as_ref();
    if !path.exists() {
        return Ok(IngestState::default());
    }

    let json = fs::read_to_string(path)
        .with_context(|| format!("failed to read ingest state {}", path.display()))?;
    serde_json::from_str(&json)
        .with_context(|| format!("failed to parse ingest state {}", path.display()))
}

pub fn save_ingest_state(path: impl AsRef<Path>, state: &IngestState) -> Result<()> {
    let path = path.as_ref();
    let parent = path
        .parent()
        .ok_or_else(|| anyhow::anyhow!("ingest state path has no parent: {}", path.display()))?;
    fs::create_dir_all(parent).with_context(|| {
        format!(
            "failed to create ingest state parent directory {}",
            parent.display()
        )
    })?;

    let json = serde_json::to_string_pretty(state)?;
    atomic_write(path, json.as_bytes())
}

pub fn load_manual_memories(path: impl AsRef<Path>) -> Result<ManualMemoriesState> {
    let path = path.as_ref();
    if !path.exists() {
        return Ok(ManualMemoriesState::default());
    }

    let json = fs::read_to_string(path)
        .with_context(|| format!("failed to read manual memories {}", path.display()))?;
    serde_json::from_str(&json)
        .with_context(|| format!("failed to parse manual memories {}", path.display()))
}

pub fn save_manual_memories(path: impl AsRef<Path>, state: &ManualMemoriesState) -> Result<()> {
    let path = path.as_ref();
    let parent = path
        .parent()
        .ok_or_else(|| anyhow::anyhow!("manual memories path has no parent: {}", path.display()))?;
    fs::create_dir_all(parent).with_context(|| {
        format!(
            "failed to create manual memories parent directory {}",
            parent.display()
        )
    })?;

    let json = serde_json::to_string_pretty(state)?;
    atomic_write(path, json.as_bytes())
}

pub fn remember_manual_memory(
    workspace_path: impl AsRef<Path>,
    text: impl Into<String>,
    priority: ManualMemoryPriority,
) -> Result<ManualMemoryFact> {
    let text = sanitize_memory_text(&text.into());
    if text.is_empty() {
        anyhow::bail!("memory text cannot be empty");
    }

    let paths = ensure_workspace_layout(workspace_path)?;
    let _writer_lock = acquire_writer_lock(&paths)?;
    let mut state = load_manual_memories(&paths.manual_memories_path)?;
    let now = Utc::now();

    if let Some(existing) = state
        .memories
        .values_mut()
        .find(|memory| memory.text == text)
    {
        existing.priority = priority;
        existing.updated_at = now;
        let updated = existing.clone();
        save_manual_memories(&paths.manual_memories_path, &state)?;
        return Ok(updated);
    }

    let fact = ManualMemoryFact {
        id: uuid::Uuid::new_v4().to_string(),
        text,
        priority,
        created_at: now,
        updated_at: now,
    };
    state.memories.insert(fact.id.clone(), fact.clone());
    save_manual_memories(&paths.manual_memories_path, &state)?;
    Ok(fact)
}

pub fn forget_manual_memory(
    workspace_path: impl AsRef<Path>,
    memory_id: impl AsRef<str>,
) -> Result<bool> {
    let memory_id = memory_id.as_ref().trim();
    if memory_id.is_empty() {
        anyhow::bail!("memory_id cannot be empty");
    }

    let paths = ensure_workspace_layout(workspace_path)?;
    let _writer_lock = acquire_writer_lock(&paths)?;
    let mut state = load_manual_memories(&paths.manual_memories_path)?;
    let removed = state.memories.remove(memory_id).is_some();
    if removed {
        save_manual_memories(&paths.manual_memories_path, &state)?;
    }
    Ok(removed)
}

pub fn list_manual_memories(workspace_path: impl AsRef<Path>) -> Result<Vec<ManualMemoryFact>> {
    let paths = workspace_memory_paths(workspace_path)?;
    let mut memories = load_manual_memories(&paths.manual_memories_path)?
        .memories
        .into_values()
        .collect::<Vec<_>>();
    memories.sort_by(|left, right| {
        right
            .priority
            .cmp(&left.priority)
            .then_with(|| right.updated_at.cmp(&left.updated_at))
            .then_with(|| left.id.cmp(&right.id))
    });
    Ok(memories)
}

pub fn invalidate_memory_doc_key(
    workspace_path: impl AsRef<Path>,
    doc_key: impl Into<String>,
    reason: Option<String>,
) -> Result<bool> {
    let doc_key = doc_key.into();
    let doc_key = doc_key.trim();
    if doc_key.is_empty() {
        anyhow::bail!("doc_key cannot be empty");
    }

    let paths = ensure_workspace_layout(workspace_path)?;
    let _writer_lock = acquire_writer_lock(&paths)?;
    let mut state = load_ingest_state(&paths.ingest_state_path)?;
    let previous = state.invalidated_doc_keys.insert(
        doc_key.to_string(),
        MemoryInvalidation {
            invalidated_at: Utc::now(),
            reason: normalize_invalidation_reason(reason),
        },
    );
    save_ingest_state(&paths.ingest_state_path, &state)?;
    Ok(previous.is_none())
}

pub fn restore_memory_doc_key(
    workspace_path: impl AsRef<Path>,
    doc_key: impl AsRef<str>,
) -> Result<bool> {
    let doc_key = doc_key.as_ref().trim();
    if doc_key.is_empty() {
        anyhow::bail!("doc_key cannot be empty");
    }

    let paths = ensure_workspace_layout(workspace_path)?;
    let _writer_lock = acquire_writer_lock(&paths)?;
    let mut state = load_ingest_state(&paths.ingest_state_path)?;
    let removed = state.invalidated_doc_keys.remove(doc_key).is_some();
    if removed {
        save_ingest_state(&paths.ingest_state_path, &state)?;
    }
    Ok(removed)
}

pub fn build_query_with_memory(query: &str, memory_context: Option<&MemoryContext>) -> String {
    let Some(retrieved_memory) = format_retrieved_memory_preview(memory_context) else {
        return query.to_string();
    };

    let mut out = String::from(
        "Use the retrieved chat memory only when it is relevant and do not treat it as authoritative if it conflicts with the current workspace state.\n\nRetrieved memory:\n",
    );
    out.push_str(&retrieved_memory);
    out.push('\n');

    out.push_str("\nUser request:\n");
    out.push_str(query.trim());
    out
}

pub fn format_retrieved_memory_preview(memory_context: Option<&MemoryContext>) -> Option<String> {
    let memory_context = memory_context?;
    let mut lines = Vec::new();

    for hit in &memory_context.hits {
        let session_name = sanitize_memory_text(&hit.session_name);
        let role = sanitize_memory_text(&hit.role);
        let timestamp = hit.timestamp.to_rfc3339();
        let snippet = sanitize_memory_text(&hit.snippet);
        if snippet.is_empty() {
            continue;
        }

        let mut line = String::from("- [session=");
        line.push_str(if session_name.is_empty() {
            "unknown"
        } else {
            session_name.as_str()
        });
        line.push_str(" role=");
        line.push_str(if role.is_empty() {
            "unknown"
        } else {
            role.as_str()
        });
        line.push_str(" time=");
        line.push_str(&timestamp);
        line.push_str("] ");
        line.push_str(&snippet);
        lines.push(line);
    }

    (!lines.is_empty()).then(|| lines.join("\n"))
}
