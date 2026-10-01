use crate::{hash, AppError, LibraryRoot, MediaItem, Ownership, Result, Tag};
use roxmltree::{Document, Node};
use std::{borrow::Cow, collections::BTreeMap, path::Path, sync::LazyLock};
// Bump whenever parsing/ownership semantics change, including validation builds.
pub const PARSER_REVISION: u32 = 5;
static TAG_COLON: LazyLock<regex::Regex> =
    LazyLock::new(|| regex::Regex::new(r"\s*:\s*").expect("constant regex"));
static TAG_RATIO: LazyLock<regex::Regex> = LazyLock::new(|| {
    regex::Regex::new(r"^(\d+(?:\.\d+)?):(\d+)(?:\s*\(.*\))?$").expect("constant regex")
});
static IMDB_ID: LazyLock<regex::Regex> =
    LazyLock::new(|| regex::Regex::new(r"^tt\d{5,12}$").expect("constant regex"));
static XML_ENCODING: LazyLock<regex::Regex> = LazyLock::new(|| {
    regex::Regex::new(r#"[\t\r\n ]encoding[\t\r\n ]*=[\t\r\n ]*(?:"([A-Za-z][A-Za-z0-9._-]*)"|'([A-Za-z][A-Za-z0-9._-]*)')"#)
        .expect("constant regex")
});
static HTML_ENTITIES: LazyLock<BTreeMap<&'static str, char>> = LazyLock::new(|| {
    include_str!("../assets/html4-entities.tsv")
        .lines()
        .filter(|line| !line.starts_with('#'))
        .map(|line| {
            let (name, code) = line.split_once('\t').expect("constant entity mapping");
            let value = u32::from_str_radix(code, 16).expect("constant entity code");
            (
                name,
                char::from_u32(value).expect("constant entity character"),
            )
        })
        .collect()
});
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
    let decoded = html_decode(value);
    let text = decoded.split_whitespace().collect::<Vec<_>>().join(" ");
    let text = crate::ownership_key(&text);
    let colon = TAG_COLON.replace_all(&text, ":").into_owned();
    TAG_RATIO
        .captures(&colon)
        .map(|c| format!("{}:{}", &c[1], &c[2]))
        .unwrap_or(colon)
}
// Match the original .NET comparator: one decode pass, mandatory semicolons,
// case-sensitive HTML 4 names, strict Unicode numeric entities, unknowns intact.
fn html_decode(value: &str) -> Cow<'_, str> {
    if !value.contains('&') {
        return Cow::Borrowed(value);
    }
    let mut output = String::with_capacity(value.len());
    let mut remaining = value;
    while let Some(start) = remaining.find('&') {
        output.push_str(&remaining[..start]);
        remaining = &remaining[start..];
        if let Some(end) = remaining[1..].find([';', '&']).map(|index| index + 1) {
            if remaining.as_bytes()[end] == b';' {
                let entity = &remaining[1..end];
                let decoded = if let Some(number) = entity.strip_prefix('#') {
                    let code = if let Some(hex) = number.strip_prefix(['x', 'X']) {
                        u32::from_str_radix(hex, 16).ok()
                    } else {
                        let number = number.trim();
                        if let Some(negative) = number.strip_prefix('-') {
                            negative.parse::<u32>().ok().filter(|code| *code == 0)
                        } else {
                            number.parse::<u32>().ok()
                        }
                    };
                    code.and_then(char::from_u32)
                } else {
                    HTML_ENTITIES.get(entity).copied()
                };
                if let Some(decoded) = decoded {
                    output.push(decoded);
                    remaining = &remaining[end + 1..];
                    continue;
                }
            }
        }
        output.push('&');
        remaining = &remaining[1..];
    }
    output.push_str(remaining);
    Cow::Owned(output)
}
pub fn parse(root: &LibraryRoot, path: &Path, raw: &[u8]) -> Result<MediaItem> {
    let source = xml_source(raw, path)?;
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
    item.original_title = direct(xml, "originaltitle");
    item.show_title = direct(xml, "showtitle");
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
            .filter(|n| {
                n.has_tag_name("uniqueid")
                    && n.attribute("type")
                        .is_some_and(|s| s.trim().eq_ignore_ascii_case("imdb"))
            })
            .map(text)
            .find(|value| !value.is_empty())
            .unwrap_or_default();
    }
    if !IMDB_ID.is_match(&item.imdb) {
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
                        .is_some_and(|s| s.trim().eq_ignore_ascii_case("IMDb Tech Manager"))
            }) {
                let engine = if ownership == Ownership::Generated {
                    manifest
                        .attribute("engine")
                        .unwrap_or("")
                        .trim()
                        .to_lowercase()
                } else {
                    String::new()
                };
                for tag in manifest.children().filter(|n| n.has_tag_name("tag")) {
                    owners.push((
                        canonical_tag(&text(tag)),
                        ownership.clone(),
                        engine.clone(),
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
// The original XmlDocument reads Unicode XML directly from bytes. Decode only
// in memory; hashing and cache identity continue to use the untouched file.
fn xml_source<'a>(raw: &'a [u8], path: &Path) -> Result<Cow<'a, str>> {
    let (width, little, bytes) = match raw {
        [0xff, 0xfe, 0, 0, ..] => (4, true, &raw[4..]),
        [0, 0, 0xfe, 0xff, ..] => (4, false, &raw[4..]),
        [0x3c, 0, 0, 0, ..] => (4, true, raw),
        [0, 0, 0, 0x3c, ..] => (4, false, raw),
        [0xff, 0xfe, ..] => (2, true, &raw[2..]),
        [0xfe, 0xff, ..] => (2, false, &raw[2..]),
        [0x3c, 0, 0x3f, 0, ..] => (2, true, raw),
        [0, 0x3c, 0, 0x3f, ..] => (2, false, raw),
        _ => return xml_declared_source(raw, path),
    };
    let invalid =
        || AppError::new("invalid-encoding", "Invalid Unicode XML bytes").at(path.display());
    if bytes.len() % width != 0 {
        return Err(invalid());
    }
    let decoded = if width == 2 {
        let words = bytes.as_chunks::<2>().0.iter().map(|pair| {
            let pair = *pair;
            if little {
                u16::from_le_bytes(pair)
            } else {
                u16::from_be_bytes(pair)
            }
        });
        char::decode_utf16(words)
            .collect::<std::result::Result<String, _>>()
            .map_err(|_| invalid())?
    } else {
        bytes
            .as_chunks::<4>()
            .0
            .iter()
            .map(|word| {
                let word = *word;
                char::from_u32(if little {
                    u32::from_le_bytes(word)
                } else {
                    u32::from_be_bytes(word)
                })
                .ok_or_else(invalid)
            })
            .collect::<Result<String>>()?
    };
    if let Some(label) = declared_encoding(&decoded) {
        let label = label.to_ascii_lowercase();
        let compatible = matches!(
            (width, little, label.as_str()),
            (2, _, "utf-16")
                | (4, _, "utf-32")
                | (2, true, "utf-16le" | "unicode")
                | (2, false, "utf-16be" | "unicodefffe")
                | (4, true, "utf-32le")
                | (4, false, "utf-32be")
        );
        if !compatible {
            return Err(invalid());
        }
    }
    Ok(Cow::Owned(decoded))
}
fn declared_encoding(source: &str) -> Option<&str> {
    if !source.starts_with("<?xml")
        || !source
            .as_bytes()
            .get(5)
            .is_some_and(|byte| matches!(byte, b' ' | b'\t' | b'\r' | b'\n'))
    {
        return None;
    }
    let end = source.find("?>")?;
    let captures = XML_ENCODING.captures(&source[..end])?;
    captures
        .get(1)
        .or_else(|| captures.get(2))
        .map(|name| name.as_str())
}
fn xml_declared_source<'a>(raw: &'a [u8], path: &Path) -> Result<Cow<'a, str>> {
    let raw = raw.strip_prefix(&[0xef, 0xbb, 0xbf]).unwrap_or(raw);
    let declaration = (raw.starts_with(b"<?xml")
        && raw
            .get(5)
            .is_some_and(|byte| matches!(byte, b' ' | b'\t' | b'\r' | b'\n')))
    .then(|| {
        raw.windows(2)
            .position(|pair| pair == b"?>")
            .and_then(|end| std::str::from_utf8(&raw[..end + 2]).ok())
    })
    .flatten();
    let label = declaration.and_then(declared_encoding);
    let invalid = || {
        AppError::new("invalid-encoding", "Invalid or unsupported XML encoding").at(path.display())
    };
    let Some(label) = label else {
        return std::str::from_utf8(raw)
            .map(Cow::Borrowed)
            .map_err(|_| invalid());
    };
    // XML's ISO-8859-1 is the actual Latin-1 mapping, not the browser alias to
    // Windows-1252 (which would change bytes in the 0x80..0x9F range).
    if matches!(
        label.to_ascii_lowercase().as_str(),
        "iso-8859-1" | "iso_8859-1" | "latin1" | "latin-1" | "l1"
    ) {
        return Ok(Cow::Owned(
            raw.iter().map(|byte| char::from(*byte)).collect(),
        ));
    }
    if matches!(label.to_ascii_lowercase().as_str(), "ascii" | "us-ascii") && !raw.is_ascii() {
        return Err(invalid());
    }
    let encoding =
        encoding_rs::Encoding::for_label_no_replacement(label.as_bytes()).ok_or_else(invalid)?;
    if encoding == encoding_rs::X_USER_DEFINED {
        return Err(invalid());
    }
    encoding
        .decode_without_bom_handling_and_without_replacement(raw)
        .ok_or_else(invalid)
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
        original_title: String::new(),
        show_title: String::new(),
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
    let raw =
        std::fs::read(&real).map_err(|e| AppError::new("read-failed", e).at(path.display()))?;
    Ok((real, raw))
}
