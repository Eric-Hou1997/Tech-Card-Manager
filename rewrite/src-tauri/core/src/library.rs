use crate::{hash, AppError, LibraryRoot, MediaItem, Ownership, Result, Tag};
use roxmltree::{Document, Node};
use std::io::Read;
use std::{collections::BTreeMap, path::Path};
pub const MAX_NFO_BYTES: u64 = 32 * 1024 * 1024;
// Bump whenever parsing/ownership semantics change, including validation builds.
pub const PARSER_REVISION: u32 = 2;
fn text(node: Node<'_, '_>) -> String {
    node.descendants()
        .filter(|n| n.is_text())
        .filter_map(|n| n.text())
        .collect::<String>()
        .trim()
        .into()
}
fn direct(root: Node<'_, '_>, name: &str) -> String {
    root.children()
        .find(|n| n.has_tag_name(name))
        .map(text)
        .unwrap_or_default()
}
pub fn canonical_tag(value: &str) -> String {
    let text = value.split_whitespace().collect::<Vec<_>>().join(" ");
    let text = crate::ownership_key(&text);
    let colon = regex::Regex::new(r"\s*:\s*")
        .expect("constant regex")
        .replace_all(&text, ":")
        .into_owned();
    let ratio =
        regex::Regex::new(r"^(\d+(?:\.\d+)?):(\d+)(?:\s*\(.*\))?$").expect("constant regex");
    ratio
        .captures(&colon)
        .map(|c| format!("{}:{}", &c[1], &c[2]))
        .unwrap_or(colon)
}
pub fn parse(root: &LibraryRoot, path: &Path, raw: &[u8]) -> Result<MediaItem> {
    let source = std::str::from_utf8(raw)
        .map_err(|e| AppError::new("invalid-encoding", e).at(path.display()))?;
    let doc = Document::parse(source.trim_start_matches('\u{feff}'))
        .map_err(|e| AppError::new("invalid-xml", e).at(path.display()))?;
    let xml = doc.root_element();
    let kind = match xml.tag_name().name().to_ascii_lowercase().as_str() {
        "movie" => "Movie",
        "tvshow" => "Series",
        "season" => "Season",
        "episodedetails" => "Episode",
        _ => {
            return Err(
                AppError::new("unsupported-nfo", "Unsupported media element").at(path.display()),
            )
        }
    };
    let mut item = empty(root, path);
    item.source_hash = hash(raw);
    item.title = direct(xml, "title");
    item.year = direct(xml, "year");
    item.kind = kind.into();
    item.season = direct(xml, "season");
    item.episode = direct(xml, "episode");
    let tech = xml.children().rfind(|n| {
        n.has_tag_name("technicalspecs")
            && n.attribute("source")
                .is_some_and(|s| s.trim().eq_ignore_ascii_case("IMDb"))
    });
    item.imdb = tech
        .and_then(|n| n.attribute("imdbid"))
        .unwrap_or("")
        .trim()
        .into();
    if item.imdb.is_empty() {
        item.imdb = xml
            .children()
            .find(|n| {
                n.has_tag_name("uniqueid")
                    && n.attribute("type")
                        .is_some_and(|s| s.eq_ignore_ascii_case("imdb"))
            })
            .map(text)
            .unwrap_or_default();
    }
    if !regex::Regex::new(r"^tt\d{5,12}$")
        .expect("constant regex")
        .is_match(&item.imdb)
    {
        item.imdb.clear();
    }
    let mut owners = vec![];
    if let Some(tech) = tech {
        for section in tech.children().filter(|n| n.has_tag_name("section")) {
            let key = section.attribute("name").unwrap_or("").trim();
            if key.is_empty() {
                continue;
            }
            let mut values = vec![];
            for value in section
                .children()
                .filter(|n| n.has_tag_name("item"))
                .map(text)
            {
                if !value.is_empty() && !values.contains(&value) {
                    values.push(value);
                }
            }
            if !values.is_empty() {
                crate::collect_specs(&mut item.specs, key, values);
            }
        }
        for (name, ownership) in [
            ("manualtags", Ownership::Manual),
            ("generatedtags", Ownership::Generated),
        ] {
            for manifest in tech.children().filter(|n| {
                n.has_tag_name(name)
                    && n.attribute("owner")
                        .is_some_and(|s| s.eq_ignore_ascii_case("IMDb Tech Manager"))
            }) {
                for tag in manifest.children().filter(|n| n.has_tag_name("tag")) {
                    owners.push((
                        canonical_tag(&text(tag)),
                        ownership.clone(),
                        manifest.attribute("engine").unwrap_or("").to_lowercase(),
                        false,
                    ));
                }
            }
        }
    }
    for value in xml
        .children()
        .filter(|n| n.has_tag_name("tag"))
        .map(text)
        .filter(|s| !s.is_empty())
    {
        let found = owners
            .iter_mut()
            .find(|o| !o.3 && o.0 == canonical_tag(&value));
        let (ownership, engine) = if let Some(o) = found {
            o.3 = true;
            (o.1.clone(), o.2.clone())
        } else {
            (Ownership::External, String::new())
        };
        item.tags.push(Tag {
            value,
            ownership,
            engine,
        });
    }
    Ok(item)
}
pub fn empty(root: &LibraryRoot, path: &Path) -> MediaItem {
    let path = path.to_string_lossy().into_owned();
    MediaItem {
        parser_revision: PARSER_REVISION,
        id: hash(path.as_bytes()),
        root_id: root.id.clone(),
        space: root.space.clone(),
        path,
        source_hash: String::new(),
        title: String::new(),
        year: String::new(),
        imdb: String::new(),
        kind: String::new(),
        season: String::new(),
        episode: String::new(),
        specs: BTreeMap::new(),
        tags: vec![],
        error: None,
    }
}
pub fn read(root: &LibraryRoot, path: &Path) -> Result<MediaItem> {
    let (real, raw) = read_bytes(root, path)?;
    parse(root, &real, &raw)
}
pub fn read_bytes(root: &LibraryRoot, path: &Path) -> Result<(std::path::PathBuf, Vec<u8>)> {
    let real = crate::paths::within(Path::new(&root.path), path)?;
    let meta =
        std::fs::metadata(&real).map_err(|e| AppError::new("read-failed", e).at(path.display()))?;
    if meta.len() > MAX_NFO_BYTES {
        return Err(
            AppError::new("nfo-too-large", "NFO exceeds 32 MiB read limit").at(path.display()),
        );
    }
    let mut raw = Vec::new();
    std::fs::File::open(&real)
        .and_then(|f| f.take(MAX_NFO_BYTES + 1).read_to_end(&mut raw))
        .map_err(|e| AppError::new("read-failed", e).at(path.display()))?;
    if raw.len() as u64 > MAX_NFO_BYTES {
        return Err(AppError::new("nfo-too-large", "NFO grew beyond read limit").at(path.display()));
    }
    Ok((real, raw))
}
