//! Port of `internal/gcimporter/exportdata.go` (`FindExportData`).

use crate::error::Error;

const ARCHIVE_SIG: &[u8] = b"!<arch>\n";
const PKGDEF: &str = "__.PKGDEF";
const EXPORT_HDR: &[u8] = b"$$B\n";
const END_OF_SECTION: &[u8] = b"\n$$\n";

/// Locate export data in a `.a` archive or raw object and return a sub-slice.
///
/// Port of `gcexportdata.NewReader` / `gcimporter.FindExportData`.
pub fn find_export_data(data: &[u8]) -> Result<&[u8], Error> {
    if !data.starts_with(ARCHIVE_SIG) {
        return Err(Error::Decode(
            "not the start of an archive file".to_string(),
        ));
    }

    let mut pos = ARCHIVE_SIG.len();
    let size = read_archive_header(data, &mut pos, PKGDEF)?;
    if size <= 0 {
        return Err(Error::Decode("not a package file".to_string()));
    }

    let pkgdef_end = pos + size;
    if pkgdef_end > data.len() {
        return Err(Error::Decode("truncated archive".to_string()));
    }

    let mut p = pos;
    let line_start = p;
    read_line(data, &mut p)?;
    let objapi = std::str::from_utf8(&data[line_start..p])
        .map_err(|e| Error::Decode(e.to_string()))?;
    if !objapi.starts_with("go object ") {
        return Err(Error::Decode(format!("not a go object file: {objapi}")));
    }

    while p < pkgdef_end {
        if data[p..].starts_with(b"$$") {
            break;
        }
        let _ = read_line(data, &mut p)?;
    }

    if !data[p..].starts_with(EXPORT_HDR) {
        let hdr = read_line(data, &mut p)?;
        let hdr = std::str::from_utf8(&data[hdr..p]).unwrap_or("<invalid>");
        return Err(Error::Decode(format!("unknown export data header: {hdr:?}")));
    }
    p += EXPORT_HDR.len();

    if p >= pkgdef_end {
        return Err(Error::Decode("missing export format byte".to_string()));
    }
    if data[p] != b'u' {
        return Err(Error::Decode(format!(
            "binary export format {:?} is no longer supported (recompile package)",
            data[p] as char
        )));
    }

    let export_end = pkgdef_end
        .checked_sub(END_OF_SECTION.len())
        .ok_or_else(|| Error::Decode("invalid export section size".to_string()))?;
    if export_end < p {
        return Err(Error::Decode(
            "invalid size in the archive file (recompile package)".to_string(),
        ));
    }

    Ok(&data[p..export_end])
}

fn read_archive_header(data: &[u8], pos: &mut usize, name: &str) -> Result<usize, Error> {
    const HEADER_SIZE: usize = 60;
    if *pos + HEADER_SIZE > data.len() {
        return Err(Error::Decode("short archive header".to_string()));
    }
    let header = &data[*pos..*pos + HEADER_SIZE];
    let hdr_name = std::str::from_utf8(&header[..16])
        .map_err(|e| Error::Decode(e.to_string()))?
        .trim_end();
    if hdr_name != name {
        return Err(Error::Decode(format!("expected archive member {name}")));
    }
    let size_str = std::str::from_utf8(&header[48..58])
        .map_err(|e| Error::Decode(e.to_string()))?
        .trim();
    let size: usize = size_str
        .parse()
        .map_err(|_| Error::Decode(format!("invalid archive size {size_str:?}")))?;
    *pos += HEADER_SIZE;
    Ok(size)
}

fn read_line(data: &[u8], pos: &mut usize) -> Result<usize, Error> {
    let start = *pos;
    let rest = &data[start..];
    let Some(rel) = rest.iter().position(|&b| b == b'\n') else {
        return Err(Error::Decode("unexpected EOF in archive".to_string()));
    };
    *pos = start + rel + 1;
    Ok(start)
}
