use super::*;
use crate::diagnostics::*;
impl Store {
    pub fn library_diagnostics(&self) -> Result<LibraryDiagnostics> {
        let mut db = self.db()?;
        let tx = db.transaction()?;
        let database = tx.query_row("PRAGMA quick_check(1)", [], |r| r.get::<_, String>(0))?;
        let config = tx
            .query_row("SELECT body FROM configuration WHERE id=1", [], |r| {
                r.get::<_, String>(0)
            })
            .optional()?
            .map(|s| serde_json::from_str::<Configuration>(&s))
            .transpose()?
            .unwrap_or_default();
        let total = tx.query_row("SELECT COUNT(*) FROM items", [], |r| r.get(0))?;
        let errors_total: u32 = tx.query_row(
            "SELECT COUNT(*) FROM items WHERE json_extract(body,'$.error') IS NOT NULL",
            [],
            |r| r.get(0),
        )?;
        let errors=tx.prepare("SELECT body FROM items WHERE json_extract(body,'$.error') IS NOT NULL ORDER BY id LIMIT 200")?.query_map([],|r|r.get::<_,String>(0))?.map(|row|{
            let item:MediaItem=serde_json::from_str(&row?)?;
            let error=item.error.ok_or_else(||AppError::new("diagnostics-state","Expected an indexed NFO error"))?;
            Ok(NfoDiagnostic{id:item.id,path:item.path,title:item.title,year:item.year,imdb:item.imdb,kind:item.kind,error})
        }).collect::<Result<Vec<_>>>()?;
        let recent_tasks = tx
            .prepare("SELECT body FROM tasks ORDER BY rowid DESC LIMIT 10")?
            .query_map([], |r| r.get::<_, String>(0))?
            .map(|row| Ok(serde_json::from_str::<Task>(&row?)?))
            .collect::<Result<Vec<_>>>()?;
        tx.commit()?;
        drop(db);
        let roots = config
            .roots
            .into_iter()
            .map(|root| {
                let checked = crate::paths::checked(Path::new(&root.path)).and_then(|path| {
                    if !path.is_dir() {
                        return Err(AppError::new(
                            "invalid-root",
                            "Library root is not a directory",
                        )
                        .at(&root.path));
                    }
                    std::fs::read_dir(path)
                        .map_err(|e| AppError::new("root-unreadable", e).at(&root.path))?;
                    Ok(())
                });
                RootDiagnostic {
                    root,
                    state: if checked.is_ok() {
                        "readable"
                    } else {
                        "unavailable"
                    }
                    .into(),
                    error: checked.err(),
                }
            })
            .collect();
        Ok(LibraryDiagnostics {
            captured_at: chrono::Utc::now().to_rfc3339(),
            database,
            configuration_revision: config.revision,
            locale: config.locale,
            roots,
            total,
            errors_total,
            errors_truncated: errors_total > 200,
            errors,
            recent_tasks,
        })
    }
}
