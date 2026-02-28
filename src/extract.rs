//! Multi-format text extraction for binary documents (PDF, OOXML).
//!
//! Conforms to [FILE_SUPPORT.md](../docs/FILE_SUPPORT.md). Extraction is pipeline-layer:
//! connectors supply bytes + content-type; this module returns plain UTF-8 text.

use std::io::Read;

/// Supported MIME types for extraction (spec §1.1).
pub const MIME_PDF: &str = "application/pdf";
pub const MIME_DOCX: &str =
    "application/vnd.openxmlformats-officedocument.wordprocessingml.document";
pub const MIME_PPTX: &str =
    "application/vnd.openxmlformats-officedocument.presentationml.presentation";
pub const MIME_XLSX: &str = "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet";

/// Maximum sheets to process in an xlsx (spec §5.2: implementation MAY limit).
const XLSX_MAX_SHEETS: usize = 100;
/// Maximum cells to process per sheet (avoids unbounded memory).
const XLSX_MAX_CELLS_PER_SHEET: usize = 100_000;
/// Maximum decompressed bytes to read from a single ZIP entry (zip-bomb protection).
const MAX_XML_ENTRY_BYTES: u64 = 50 * 1024 * 1024;

/// Extraction error (spec §5.1: no panic; return error and pipeline skips item).
#[derive(Debug)]
pub enum ExtractError {
    UnsupportedContentType(String),
    Pdf(String),
    Ooxml(String),
}

impl std::fmt::Display for ExtractError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ExtractError::UnsupportedContentType(ct) => {
                write!(f, "unsupported content-type: {}", ct)
            }
            ExtractError::Pdf(e) => write!(f, "PDF extraction failed: {}", e),
            ExtractError::Ooxml(e) => write!(f, "OOXML extraction failed: {}", e),
        }
    }
}

impl std::error::Error for ExtractError {}

/// Extracts plain text from binary content. Returns UTF-8 string or error (spec §5, §6).
pub fn extract_text(bytes: &[u8], content_type: &str) -> Result<String, ExtractError> {
    match content_type {
        MIME_PDF => extract_pdf(bytes),
        MIME_DOCX => extract_docx(bytes),
        MIME_PPTX => extract_pptx(bytes),
        MIME_XLSX => extract_xlsx(bytes),
        _ => Err(ExtractError::UnsupportedContentType(
            content_type.to_string(),
        )),
    }
}

fn extract_pdf(bytes: &[u8]) -> Result<String, ExtractError> {
    pdf_extract::extract_text_from_mem(bytes).map_err(|e| ExtractError::Pdf(e.to_string()))
}

fn read_zip_entry_bounded(
    archive: &mut zip::ZipArchive<std::io::Cursor<&[u8]>>,
    name: &str,
    max_bytes: u64,
) -> Result<Vec<u8>, ExtractError> {
    let entry = archive
        .by_name(name)
        .map_err(|e| ExtractError::Ooxml(e.to_string()))?;
    let mut out = Vec::new();
    entry
        .take(max_bytes)
        .read_to_end(&mut out)
        .map_err(|e| ExtractError::Ooxml(e.to_string()))?;
    if out.len() as u64 >= max_bytes {
        return Err(ExtractError::Ooxml(format!(
            "ZIP entry {} exceeds size limit ({} bytes)",
            name, max_bytes
        )));
    }
    Ok(out)
}

fn extract_docx(bytes: &[u8]) -> Result<String, ExtractError> {
    let mut archive = zip::ZipArchive::new(std::io::Cursor::new(bytes))
        .map_err(|e| ExtractError::Ooxml(e.to_string()))?;
    let mut doc_xml = Vec::new();
    let mut found = false;
    for i in 0..archive.len() {
        let entry = archive
            .by_index(i)
            .map_err(|e| ExtractError::Ooxml(e.to_string()))?;
        if entry.name() == "word/document.xml" {
            entry
                .take(MAX_XML_ENTRY_BYTES)
                .read_to_end(&mut doc_xml)
                .map_err(|e| ExtractError::Ooxml(e.to_string()))?;
            if doc_xml.len() as u64 >= MAX_XML_ENTRY_BYTES {
                return Err(ExtractError::Ooxml(
                    "word/document.xml exceeds size limit".to_string(),
                ));
            }
            found = true;
            break;
        }
    }
    if !found {
        return Err(ExtractError::Ooxml(
            "word/document.xml not found".to_string(),
        ));
    }
    extract_w_t_elements(&doc_xml)
}

fn extract_w_t_elements(xml: &[u8]) -> Result<String, ExtractError> {
    let mut out = String::new();
    let mut reader = quick_xml::Reader::from_reader(xml);
    reader.config_mut().trim_text(true);
    let mut buf = Vec::new();
    loop {
        match reader.read_event_into(&mut buf) {
            Ok(quick_xml::events::Event::Start(e)) => {
                let name = e.local_name();
                if name.as_ref() == b"t" {
                    if let Ok(quick_xml::events::Event::Text(te)) = reader.read_event_into(&mut buf)
                    {
                        out.push_str(te.unescape().unwrap_or_default().as_ref());
                    }
                }
            }
            Ok(quick_xml::events::Event::Empty(e)) => {
                if e.local_name().as_ref() == b"t" {
                    // empty t, nothing to add
                }
            }
            Ok(quick_xml::events::Event::Eof) => break,
            Err(e) => return Err(ExtractError::Ooxml(e.to_string())),
            _ => {}
        }
        buf.clear();
    }
    Ok(out)
}

fn extract_pptx(bytes: &[u8]) -> Result<String, ExtractError> {
    let mut archive = zip::ZipArchive::new(std::io::Cursor::new(bytes))
        .map_err(|e| ExtractError::Ooxml(e.to_string()))?;
    let mut slide_names: Vec<String> = archive
        .file_names()
        .filter(|n| n.starts_with("ppt/slides/slide") && n.ends_with(".xml"))
        .map(|s| s.to_string())
        .collect();
    slide_names.sort_by_key(|name| {
        name.trim_start_matches("ppt/slides/slide")
            .trim_end_matches(".xml")
            .parse::<u32>()
            .unwrap_or(u32::MAX)
    });
    let mut out = String::new();
    for name in slide_names {
        let xml = read_zip_entry_bounded(&mut archive, &name, MAX_XML_ENTRY_BYTES)?;
        let text = extract_a_t_elements(&xml)?;
        if !out.is_empty() && !text.is_empty() {
            out.push(' ');
        }
        out.push_str(&text);
    }
    Ok(out)
}

fn extract_a_t_elements(xml: &[u8]) -> Result<String, ExtractError> {
    let mut out = String::new();
    let mut reader = quick_xml::Reader::from_reader(xml);
    reader.config_mut().trim_text(true);
    let mut buf = Vec::new();
    loop {
        match reader.read_event_into(&mut buf) {
            Ok(quick_xml::events::Event::Start(e)) => {
                if e.local_name().as_ref() == b"t" {
                    if let Ok(quick_xml::events::Event::Text(te)) = reader.read_event_into(&mut buf)
                    {
                        out.push_str(te.unescape().unwrap_or_default().as_ref());
                    }
                }
            }
            Ok(quick_xml::events::Event::Eof) => break,
            Err(e) => return Err(ExtractError::Ooxml(e.to_string())),
            _ => {}
        }
        buf.clear();
    }
    Ok(out)
}

fn extract_xlsx(bytes: &[u8]) -> Result<String, ExtractError> {
    let mut archive = zip::ZipArchive::new(std::io::Cursor::new(bytes))
        .map_err(|e| ExtractError::Ooxml(e.to_string()))?;
    let shared_strings = read_shared_strings(&mut archive)?;
    let sheet_names = list_worksheet_names(&mut archive)?;
    let mut out = String::new();
    for (idx, name) in sheet_names.into_iter().take(XLSX_MAX_SHEETS).enumerate() {
        let sheet_xml = read_zip_entry_bounded(&mut archive, &name, MAX_XML_ENTRY_BYTES)?;
        let cell_texts = extract_xlsx_sheet_cells(&sheet_xml, &shared_strings)?;
        if idx > 0 && !out.is_empty() {
            out.push(' ');
        }
        out.push_str(&cell_texts);
    }
    Ok(out)
}

fn read_shared_strings(
    archive: &mut zip::ZipArchive<std::io::Cursor<&[u8]>>,
) -> Result<Vec<String>, ExtractError> {
    let xml = read_zip_entry_bounded(archive, "xl/sharedStrings.xml", MAX_XML_ENTRY_BYTES)?;
    let mut strings = Vec::new();
    let mut reader = quick_xml::Reader::from_reader(xml.as_slice());
    reader.config_mut().trim_text(true);
    let mut buf = Vec::new();
    let mut in_si = false;
    loop {
        match reader.read_event_into(&mut buf) {
            Ok(quick_xml::events::Event::Start(e)) => {
                if e.local_name().as_ref() == b"si" {
                    in_si = true;
                } else if in_si && e.local_name().as_ref() == b"t" {
                    if let Ok(quick_xml::events::Event::Text(te)) = reader.read_event_into(&mut buf)
                    {
                        strings.push(te.unescape().unwrap_or_default().into_owned());
                    }
                }
            }
            Ok(quick_xml::events::Event::End(e)) => {
                if e.local_name().as_ref() == b"si" {
                    in_si = false;
                }
            }
            Ok(quick_xml::events::Event::Eof) => break,
            Err(e) => return Err(ExtractError::Ooxml(e.to_string())),
            _ => {}
        }
        buf.clear();
    }
    Ok(strings)
}

fn list_worksheet_names(
    archive: &mut zip::ZipArchive<std::io::Cursor<&[u8]>>,
) -> Result<Vec<String>, ExtractError> {
    let mut names: Vec<String> = archive
        .file_names()
        .filter(|n| n.starts_with("xl/worksheets/sheet") && n.ends_with(".xml"))
        .map(|s| s.to_string())
        .collect();
    names.sort_by_key(|name| {
        name.trim_start_matches("xl/worksheets/sheet")
            .trim_end_matches(".xml")
            .parse::<u32>()
            .unwrap_or(u32::MAX)
    });
    Ok(names)
}

fn extract_xlsx_sheet_cells(xml: &[u8], shared_strings: &[String]) -> Result<String, ExtractError> {
    let mut cells: Vec<String> = Vec::new();
    let mut reader = quick_xml::Reader::from_reader(xml);
    reader.config_mut().trim_text(true);
    let mut buf = Vec::new();
    let mut in_v = false;
    let mut cell_is_shared_str = false;
    let mut cell_count = 0usize;
    loop {
        if cell_count >= XLSX_MAX_CELLS_PER_SHEET {
            break;
        }
        match reader.read_event_into(&mut buf) {
            Ok(quick_xml::events::Event::Start(e)) => {
                if e.local_name().as_ref() == b"c" {
                    cell_is_shared_str = e.attributes().any(|a| {
                        a.as_ref()
                            .map(|a| a.key.as_ref() == b"t" && a.value.as_ref() == b"s")
                            .unwrap_or(false)
                    });
                } else if e.local_name().as_ref() == b"v" {
                    in_v = true;
                }
            }
            Ok(quick_xml::events::Event::Text(te)) if in_v => {
                let v = te.unescape().unwrap_or_default();
                let s = v.trim();
                if !s.is_empty() && cell_is_shared_str {
                    if let Ok(i) = s.parse::<usize>() {
                        if i < shared_strings.len() {
                            cells.push(shared_strings[i].clone());
                            cell_count += 1;
                        }
                    }
                }
                in_v = false;
            }
            Ok(quick_xml::events::Event::End(e)) => {
                if e.local_name().as_ref() == b"v" {
                    in_v = false;
                } else if e.local_name().as_ref() == b"c" {
                    cell_is_shared_str = false;
                }
            }
            Ok(quick_xml::events::Event::Eof) => break,
            Err(e) => return Err(ExtractError::Ooxml(e.to_string())),
            _ => {}
        }
        buf.clear();
    }
    Ok(cells.join(" "))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unsupported_content_type_returns_error() {
        let err = extract_text(b"foo", "application/octet-stream").unwrap_err();
        assert!(matches!(err, ExtractError::UnsupportedContentType(_)));
    }

    #[test]
    fn invalid_pdf_returns_error() {
        let err = extract_text(b"not a pdf", MIME_PDF).unwrap_err();
        assert!(matches!(err, ExtractError::Pdf(_)));
    }

    #[test]
    fn invalid_zip_returns_error_for_docx() {
        let err = extract_text(b"not a zip", MIME_DOCX).unwrap_err();
        assert!(matches!(err, ExtractError::Ooxml(_)));
    }
}
