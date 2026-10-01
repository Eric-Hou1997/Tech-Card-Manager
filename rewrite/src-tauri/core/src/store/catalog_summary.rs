use super::*;
impl Store {
    pub fn catalog_summary(&self) -> crate::Result<crate::ui::CatalogSummary> {
        // Read this before holding the catalog connection guard. A corrupt
        // completed summary is a visible status error in v4.1.0, while the
        // independently readable catalog remains usable.
        let index_error = self.index_summary().err();
        let db = self.db()?;
        let items = db
            .prepare("SELECT body FROM items ORDER BY id")?
            .query_map([], |row| row.get::<_, String>(0))?
            .map(|row| Ok(serde_json::from_str::<MediaItem>(&row?)?))
            .collect::<crate::Result<Vec<_>>>()?;
        let time: Option<String> = db
            .query_row(
                "SELECT body FROM preferences WHERE key='catalog-generated-at'",
                [],
                |row| row.get(0),
            )
            .optional()?;
        let mut summary = crate::ui::summary(&items);
        summary.generated_at = time.map(|value| serde_json::from_str(&value)).transpose()?;
        summary.roots_configured = db.query_row("SELECT EXISTS(SELECT 1 FROM preferences WHERE key='media-folders') OR EXISTS(SELECT 1 FROM configuration WHERE json_array_length(body,'$.roots')>0)", [], |row| row.get(0))?;
        summary.index_error = index_error;
        Ok(summary)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn corrupt_completed_summary_is_visible_without_hiding_the_catalog() {
        let temp = tempfile::tempdir().unwrap();
        let store = Store::open(&temp.path().join("state.sqlite")).unwrap();
        store
            .db()
            .unwrap()
            .execute(
                "INSERT INTO preferences(key,body) VALUES('manager-index-summary','not-json')",
                [],
            )
            .unwrap();
        let summary = store.catalog_summary().unwrap();
        assert_eq!(summary.total, 0);
        assert!(summary.index_error.is_some());
    }
}
