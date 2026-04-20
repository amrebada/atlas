//! Collection IPC commands - baseline from **D2**, extended in **D4**

// P4 wires the new member commands into `tauri::generate_handler![...]`
#![allow(dead_code)]

use crate::storage::types::{Collection, Project};
use crate::storage::Db;

/// `collections.list` - all collections ordered by `ord` ascending.
#[tauri::command]
pub async fn collections_list(state: tauri::State<'_, Db>) -> Result<Vec<Collection>, String> {
    state
        .list_collections()
        .await
        .map_err(|e: anyhow::Error| e.to_string())
}

/// `collections.upsert` - insert or update by id. Writes `label`, `dot`
#[tauri::command]
pub async fn collections_upsert(
    state: tauri::State<'_, Db>,
    collection: Collection,
) -> Result<(), String> {
    state
        .upsert_collection(&collection)
        .await
        .map_err(|e: anyhow::Error| e.to_string())
}

/// `collections.remove` - delete by id. Cascades to `collection_members`.
#[tauri::command]
pub async fn collections_remove(state: tauri::State<'_, Db>, id: String) -> Result<(), String> {
    state
        .remove_collection(&id)
        .await
        .map_err(|e: anyhow::Error| e.to_string())
}

/// `collections.members` - project ids currently assigned to a
#[tauri::command]
pub async fn collections_members(
    state: tauri::State<'_, Db>,
    id: String,
) -> Result<Vec<String>, String> {
    state
        .list_collection_members(&id)
        .await
        .map_err(|e: anyhow::Error| e.to_string())
}

/// `collections.set_members` - replace the member set of a collection
#[tauri::command]
pub async fn collections_set_members(
    state: tauri::State<'_, Db>,
    id: String,
    project_ids: Vec<String>,
) -> Result<(), String> {
    state
        .set_collection_members(&id, &project_ids)
        .await
        .map_err(|e: anyhow::Error| e.to_string())
}

// ----------------------------

/// `collections.create` - insert a new collection with a fresh uuid v4.
#[tauri::command]
pub async fn collections_create(
    state: tauri::State<'_, Db>,
    label: String,
    color: Option<String>,
) -> Result<Collection, String> {
    state
        .create_collection(&label, color.as_deref())
        .await
        .map_err(|e: anyhow::Error| e.to_string())
}

/// `collections.rename` - update just the `label` column.
#[tauri::command]
pub async fn collections_rename(
    state: tauri::State<'_, Db>,
    id: String,
    label: String,
) -> Result<(), String> {
    state
        .rename_collection(&id, &label)
        .await
        .map_err(|e: anyhow::Error| e.to_string())
}

/// `collections.update_color` - update just the `dot` swatch.
#[tauri::command]
pub async fn collections_update_color(
    state: tauri::State<'_, Db>,
    id: String,
    color: String,
) -> Result<(), String> {
    state
        .update_collection_color(&id, &color)
        .await
        .map_err(|e: anyhow::Error| e.to_string())
}

/// `collections.delete` - delete the row + every membership entry in
#[tauri::command]
pub async fn collections_delete(state: tauri::State<'_, Db>, id: String) -> Result<(), String> {
    state
        .delete_collection(&id)
        .await
        .map_err(|e: anyhow::Error| e.to_string())
}

/// `collections.reorder` - rewrite `ord` so rows sort in the given
#[tauri::command]
pub async fn collections_reorder(
    state: tauri::State<'_, Db>,
    ordered_ids: Vec<String>,
) -> Result<(), String> {
    state
        .reorder_collections(&ordered_ids)
        .await
        .map_err(|e: anyhow::Error| e.to_string())
}

/// `collections.add_project` - idempotent upsert into the link table.
#[tauri::command]
pub async fn collections_add_project(
    state: tauri::State<'_, Db>,
    project_id: String,
    collection_id: String,
) -> Result<(), String> {
    state
        .add_project_to_collection(&project_id, &collection_id)
        .await
        .map_err(|e: anyhow::Error| e.to_string())
}

/// `collections.remove_project` - idempotent delete from the link table.
#[tauri::command]
pub async fn collections_remove_project(
    state: tauri::State<'_, Db>,
    project_id: String,
    collection_id: String,
) -> Result<(), String> {
    state
        .remove_project_from_collection(&project_id, &collection_id)
        .await
        .map_err(|e: anyhow::Error| e.to_string())
}

/// `collections.projects` - hydrate every project assigned to a
#[tauri::command]
pub async fn collections_projects(
    state: tauri::State<'_, Db>,
    collection_id: String,
) -> Result<Vec<Project>, String> {
    state
        .list_collection_projects(&collection_id)
        .await
        .map_err(|e: anyhow::Error| e.to_string())
}

// ================================================================

#[cfg(test)]
mod tests {
    use super::*;

    /// Create a collection, verify it's listed and that fields came back
    #[tokio::test]
    async fn create_lists_the_new_collection() -> anyhow::Result<()> {
        let db = Db::open_in_memory().await?;

        let created = db.create_collection("Alpha", None).await?;
        assert_eq!(created.label, "Alpha");
        assert!(!created.id.is_empty(), "uuid id should be populated");
        assert!(!created.dot.is_empty(), "default color should be picked");

        let listed = db.list_collections().await?;
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].id, created.id);
        assert_eq!(listed[0].label, "Alpha");
        Ok(())
    }

    /// Override color wins over the rotating palette.
    #[tokio::test]
    async fn create_honours_explicit_color() -> anyhow::Result<()> {
        let db = Db::open_in_memory().await?;
        let c = db.create_collection("Beta", Some("#ff00aa")).await?;
        assert_eq!(c.dot, "#ff00aa");
        Ok(())
    }

    /// Reorder rewrites `ord` so list order matches caller's order.
    #[tokio::test]
    async fn reorder_sets_ord_in_caller_order() -> anyhow::Result<()> {
        let db = Db::open_in_memory().await?;
        let a = db.create_collection("A", None).await?;
        let b = db.create_collection("B", None).await?;
        let c = db.create_collection("C", None).await?;

        // Reverse: c, b, a.
        db.reorder_collections(&[c.id.clone(), b.id.clone(), a.id.clone()])
            .await?;

        let listed = db.list_collections().await?;
        assert_eq!(listed.len(), 3);
        assert_eq!(listed[0].id, c.id);
        assert_eq!(listed[0].order, 0);
        assert_eq!(listed[1].id, b.id);
        assert_eq!(listed[1].order, 1);
        assert_eq!(listed[2].id, a.id);
        assert_eq!(listed[2].order, 2);
        Ok(())
    }

    /// Add two projects to a collection and list them through the
    #[tokio::test]
    async fn add_project_and_list_roundtrip() -> anyhow::Result<()> {
        let db = Db::open_in_memory().await?;
        db.seed_fixtures().await?;
        let coll = db.create_collection("Focus", None).await?;

        db.add_project_to_collection("acorn", &coll.id).await?;
        db.add_project_to_collection("birch", &coll.id).await?;

        // Idempotent - re-add is a no-op.
        db.add_project_to_collection("acorn", &coll.id).await?;

        let projs = db.list_collection_projects(&coll.id).await?;
        assert_eq!(projs.len(), 2);
        let ids: std::collections::HashSet<&str> = projs.iter().map(|p| p.id.as_str()).collect();
        assert!(ids.contains("acorn"));
        assert!(ids.contains("birch"));
        Ok(())
    }

    /// Remove one project - list size drops by one and the correct row
    #[tokio::test]
    async fn remove_project_shrinks_list() -> anyhow::Result<()> {
        let db = Db::open_in_memory().await?;
        db.seed_fixtures().await?;
        let coll = db.create_collection("Focus", None).await?;
        db.add_project_to_collection("acorn", &coll.id).await?;
        db.add_project_to_collection("birch", &coll.id).await?;

        db.remove_project_from_collection("acorn", &coll.id).await?;

        // Idempotent - re-remove is a no-op.
        db.remove_project_from_collection("acorn", &coll.id).await?;

        let projs = db.list_collection_projects(&coll.id).await?;
        assert_eq!(projs.len(), 1);
        assert_eq!(projs[0].id, "birch");
        Ok(())
    }

    /// Deleting the collection prunes every link-table row too.
    #[tokio::test]
    async fn delete_collection_drops_link_rows() -> anyhow::Result<()> {
        let db = Db::open_in_memory().await?;
        db.seed_fixtures().await?;
        let coll = db.create_collection("Focus", None).await?;
        db.add_project_to_collection("acorn", &coll.id).await?;
        db.add_project_to_collection("birch", &coll.id).await?;

        db.delete_collection(&coll.id).await?;

        // Collection gone.
        let cols = db.list_collections().await?;
        assert!(cols.iter().all(|c| c.id != coll.id));

        // Link rows gone (direct count on the table - not using
        let (remaining,): (i64,) =
            sqlx::query_as("SELECT COUNT(*) FROM collection_members WHERE collection_id = ?")
                .bind(&coll.id)
                .fetch_one(db.pool())
                .await?;
        assert_eq!(remaining, 0);

        // And neither project now claims membership in the gone id.
        let acorn = db.get_project("acorn").await?.unwrap();
        assert!(!acorn.collection_ids.iter().any(|c| c == &coll.id));
        Ok(())
    }

    /// Rename + recolor update only their column.
    #[tokio::test]
    async fn rename_and_recolor_update_columns() -> anyhow::Result<()> {
        let db = Db::open_in_memory().await?;
        let c = db.create_collection("Old", Some("#111")).await?;

        db.rename_collection(&c.id, "New").await?;
        db.update_collection_color(&c.id, "#222").await?;

        let listed = db.list_collections().await?;
        let row = listed.iter().find(|x| x.id == c.id).unwrap();
        assert_eq!(row.label, "New");
        assert_eq!(row.dot, "#222");
        Ok(())
    }

    /// Unknown ids surface as errors for rename / recolor.
    #[tokio::test]
    async fn rename_unknown_id_errors() -> anyhow::Result<()> {
        let db = Db::open_in_memory().await?;
        let err = db
            .rename_collection("ghost", "Oops")
            .await
            .unwrap_err()
            .to_string();
        assert!(err.contains("ghost"), "err should mention id: {err}");
        Ok(())
    }
}
