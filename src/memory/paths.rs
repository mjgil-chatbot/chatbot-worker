use super::*;

pub fn memory_enabled() -> bool {
    config::enable_workspace_memory()
}

pub fn memory_root_dir() -> PathBuf {
    config::shared_infra_dir().join(MEMORY_DIR_NAME)
}

pub fn workspace_id_for_path(workspace_path: impl AsRef<Path>) -> Result<String> {
    let canonical = canonical_workspace_path(workspace_path)?;
    Ok(format!(
        "{:016x}",
        fnv_hash_bytes(FNV_OFFSET_BASIS, canonical.to_string_lossy().as_bytes())
    ))
}

pub fn workspace_memory_paths(workspace_path: impl AsRef<Path>) -> Result<WorkspaceMemoryPaths> {
    let workspace_root = canonical_workspace_path(workspace_path)?;
    let workspace_id = workspace_id_for_path(&workspace_root)?;
    let root_dir = memory_root_dir().join(&workspace_id);

    Ok(WorkspaceMemoryPaths {
        workspace_id,
        workspace_root,
        ingest_state_path: root_dir.join(INGEST_STATE_FILE_NAME),
        manual_memories_path: root_dir.join(MANUAL_MEMORIES_FILE_NAME),
        agent_memory_store_path: root_dir.join(AGENT_MEMORY_STORE_FILE_NAME),
        writer_lock_path: root_dir.join(WRITER_LOCK_FILE_NAME),
        root_dir,
    })
}

pub fn ensure_workspace_layout(workspace_path: impl AsRef<Path>) -> Result<WorkspaceMemoryPaths> {
    let paths = workspace_memory_paths(workspace_path)?;
    fs::create_dir_all(&paths.root_dir).with_context(|| {
        format!(
            "failed to create workspace memory directory {}",
            paths.root_dir.display()
        )
    })?;
    Ok(paths)
}
