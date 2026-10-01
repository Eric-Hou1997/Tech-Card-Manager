use super::*;
use crate::history::*;
impl Store {
    pub fn diagnostic_logs(&self) -> Result<Vec<DiagnosticLog>> {
        let db = self.db()?;
        let mut statement = db.prepare("SELECT import_id,path,sha256,length(body),body,modified_unix FROM legacy_artifacts WHERE category='history' AND path IN ('manager.log','agent.log','job.log','logs/manager.log','logs/agent.log','logs/job.log') ORDER BY rowid DESC")?;
        let mut names = std::collections::BTreeSet::new();
        let mut files = Vec::new();
        for row in statement.query_map([], |r| {
            Ok((
                HistoryArchive {
                    import_id: r.get(0)?,
                    path: r.get(1)?,
                    sha256: r.get(2)?,
                    bytes: r.get(3)?,
                },
                r.get::<_, Vec<u8>>(4)?,
                r.get::<_, Option<i64>>(5)?,
            ))
        })? {
            let (archive, bytes, modified_unix) = row?;
            let name = archive
                .path
                .rsplit('/')
                .next()
                .unwrap_or_default()
                .to_owned();
            if !names.insert(name) {
                continue;
            }
            if hash(&bytes) != archive.sha256 {
                return Err(AppError::new(
                    "history-integrity",
                    "Historical diagnostic log failed its original checksum",
                )
                .at(&archive.path));
            }
            files.push((archive, bytes, modified_unix));
        }
        Ok(files)
    }

    pub fn history_archives(&self, offset: u32, limit: u32) -> Result<HistoryArchives> {
        if !(1..=100).contains(&limit) {
            return Err(AppError::new(
                "history-limit",
                "Choose 1–100 historical files per page",
            ));
        }
        let db = self.db()?;
        let total = db.query_row(
            &format!("SELECT COUNT(*) FROM legacy_artifacts WHERE {ALLOWED}"),
            [],
            |r| r.get(0),
        )?;
        let items = db.prepare(&format!("SELECT import_id,path,sha256,length(body) FROM legacy_artifacts WHERE {ALLOWED} ORDER BY import_id,path LIMIT ?1 OFFSET ?2"))?.query_map(params![limit,offset], |r| Ok(HistoryArchive{import_id:r.get(0)?,path:r.get(1)?,sha256:r.get(2)?,bytes:r.get(3)?}))?.collect::<std::result::Result<_,_>>()?;
        Ok(HistoryArchives { total, items })
    }
    pub fn history_page(&self, import_id: &str, path: &str, offset: u32) -> Result<HistoryPage> {
        let db = self.db()?;
        let (archive, raw) = db.query_row(&format!("SELECT import_id,path,sha256,length(body),body FROM legacy_artifacts WHERE import_id=?1 AND path=?2 AND {ALLOWED}"), params![import_id,path], |r|Ok((HistoryArchive{import_id:r.get(0)?,path:r.get(1)?,sha256:r.get(2)?,bytes:r.get(3)?},r.get::<_,Vec<u8>>(4)?))).optional()?.ok_or_else(||AppError::new("history-not-found", "No imported history exists at this location").at(path))?;
        crate::history::page(archive, raw, offset)
    }
}
