//! Structural chunking helpers (spec §6.5).

use crate::types::{ChunkBudget, CodeChunk, Symbol, SymbolKind};
use ctx_core::line_range;

/// Build symbol-aware chunks from a sorted list of top-level symbol ranges,
/// filling gaps (imports, module-level statements) with "section" chunks and
/// subdividing oversized symbols along line boundaries.
pub fn chunk_by_symbols(source: &str, symbols: &[Symbol], budget: ChunkBudget) -> Vec<CodeChunk> {
    let mut chunks = Vec::new();
    // Only top-level (non-nested) symbols define chunk boundaries; nested
    // methods stay inside their class chunk unless the class is too large.
    let mut top: Vec<&Symbol> = symbols.iter().filter(|s| s.parent.is_none()).collect();
    top.sort_by_key(|s| (s.start_byte, std::cmp::Reverse(s.end_byte)));
    // Remove overlaps (keep the outermost).
    let mut ranges: Vec<&Symbol> = Vec::new();
    for s in top {
        if let Some(last) = ranges.last() {
            if s.start_byte < last.end_byte {
                continue;
            }
        }
        ranges.push(s);
    }

    let mut cursor = 0usize;
    for sym in ranges {
        let start = sym.start_byte.min(source.len());
        let end = sym.end_byte.min(source.len()).max(start);
        if start > cursor {
            push_gap(source, cursor, start, budget, &mut chunks);
        }
        if end - start > budget.max_bytes {
            // Oversized: split along children if any, else by lines.
            let children: Vec<&Symbol> = symbols.iter().filter(|c| c.parent.as_deref() == Some(sym.name.as_str()) && c.start_byte >= start && c.end_byte <= end).collect();
            if children.len() > 1 {
                split_container(source, sym, &children, budget, &mut chunks);
            } else {
                split_by_lines(source, start, end, Some(sym), budget, &mut chunks);
            }
        } else {
            chunks.push(make_chunk(source, start, end, Some(sym.name.clone()), Some(sym.kind), 0));
        }
        cursor = end;
    }
    if cursor < source.len() {
        push_gap(source, cursor, source.len(), budget, &mut chunks);
    }
    chunks.retain(|c| !c.content.trim().is_empty());
    chunks
}

fn push_gap(source: &str, start: usize, end: usize, budget: ChunkBudget, out: &mut Vec<CodeChunk>) {
    let start = align_to_char(source, start);
    let end = align_to_char(source, end);
    if end <= start {
        return;
    }
    let text = &source[start..end];
    if text.trim().is_empty() {
        return;
    }
    if end - start <= budget.max_bytes {
        out.push(make_chunk(source, start, end, None, Some(SymbolKind::Section), 0));
    } else {
        split_by_lines(source, start, end, None, budget, out);
    }
}

fn split_container(source: &str, sym: &Symbol, children: &[&Symbol], budget: ChunkBudget, out: &mut Vec<CodeChunk>) {
    let start = sym.start_byte;
    let end = sym.end_byte.min(source.len());
    let mut cursor = start;
    let mut part = 0;
    let mut sorted: Vec<&Symbol> = children.to_vec();
    sorted.sort_by_key(|c| c.start_byte);
    for child in sorted {
        let cs = child.start_byte.max(cursor);
        let ce = child.end_byte.min(end);
        if cs > cursor {
            // Header or inter-member text stays attached to the container.
            let header_end = cs;
            if header_end - cursor >= budget.min_bytes || part == 0 {
                out.push(make_chunk(source, cursor, header_end, Some(sym.name.clone()), Some(sym.kind), part));
                part += 1;
            }
        }
        if ce > cs {
            if ce - cs > budget.max_bytes {
                split_by_lines(source, cs, ce, Some(child), budget, out);
            } else {
                let name = format!("{}.{}", sym.name, child.name);
                out.push(make_chunk(source, cs, ce, Some(name), Some(child.kind), 0));
            }
            cursor = ce;
        }
    }
    if cursor < end {
        out.push(make_chunk(source, cursor, end, Some(sym.name.clone()), Some(sym.kind), part));
    }
}

fn split_by_lines(source: &str, start: usize, end: usize, sym: Option<&Symbol>, budget: ChunkBudget, out: &mut Vec<CodeChunk>) {
    let mut part = 0;
    let mut chunk_start = start;
    let mut pos = start;
    let text = &source[start..end];
    for line in text.split_inclusive('\n') {
        let line_end = pos + line.len();
        if line_end - chunk_start > budget.max_bytes && pos > chunk_start {
            out.push(make_chunk(source, chunk_start, pos, sym.map(|s| s.name.clone()), sym.map(|s| s.kind).or(Some(SymbolKind::Section)), part));
            part += 1;
            chunk_start = pos;
        }
        pos = line_end;
    }
    if end > chunk_start {
        out.push(make_chunk(source, chunk_start, end, sym.map(|s| s.name.clone()), sym.map(|s| s.kind).or(Some(SymbolKind::Section)), part));
    }
}

fn align_to_char(source: &str, mut idx: usize) -> usize {
    idx = idx.min(source.len());
    while idx > 0 && !source.is_char_boundary(idx) {
        idx -= 1;
    }
    idx
}

pub fn make_chunk(source: &str, start: usize, end: usize, symbol: Option<String>, kind: Option<SymbolKind>, part: usize) -> CodeChunk {
    let start = align_to_char(source, start);
    let end = align_to_char(source, end).max(start);
    let (start_line, end_line) = line_range(source, start, end);
    CodeChunk { symbol, symbol_kind: kind, start_byte: start, end_byte: end, start_line, end_line, content: source[start..end].to_string(), part }
}

/// Line-based chunking for files without structure.
pub fn chunk_by_lines(source: &str, budget: ChunkBudget) -> Vec<CodeChunk> {
    let mut out = Vec::new();
    if source.trim().is_empty() {
        return out;
    }
    split_by_lines(source, 0, source.len(), None, budget, &mut out);
    out.retain(|c| !c.content.trim().is_empty());
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sym(name: &str, start: usize, end: usize, parent: Option<&str>) -> Symbol {
        Symbol { name: name.into(), kind: SymbolKind::Function, parent: parent.map(|p| p.into()), start_byte: start, end_byte: end, start_line: 0, end_line: 0, signature: None }
    }

    #[test]
    fn gaps_and_symbols_cover_file() {
        let src = "use a;\n\nfn one() {}\n\nfn two() {}\n// trailing\n";
        let one_start = src.find("fn one").unwrap();
        let one_end = one_start + "fn one() {}".len();
        let two_start = src.find("fn two").unwrap();
        let two_end = two_start + "fn two() {}".len();
        let chunks = chunk_by_symbols(src, &[sym("one", one_start, one_end, None), sym("two", two_start, two_end, None)], ChunkBudget::default());
        let names: Vec<Option<String>> = chunks.iter().map(|c| c.symbol.clone()).collect();
        assert_eq!(names, vec![None, Some("one".into()), Some("two".into()), None]);
        assert_eq!(chunks[1].content, "fn one() {}");
        assert_eq!(chunks[1].start_line, 3);
    }

    #[test]
    fn oversized_symbol_is_split_by_lines() {
        let body: String = (0..200).map(|i| format!("    line{i};\n")).collect();
        let src = format!("fn big() {{\n{body}}}\n");
        let chunks = chunk_by_symbols(&src, &[sym("big", 0, src.len() - 1, None)], ChunkBudget { max_bytes: 500, min_bytes: 50 });
        assert!(chunks.len() > 3);
        assert!(chunks.iter().all(|c| c.symbol.as_deref() == Some("big")));
        assert!(chunks.iter().all(|c| c.content.len() <= 520));
        let joined: String = chunks.iter().map(|c| c.content.as_str()).collect();
        assert_eq!(joined, src.trim_end_matches('\n'));
    }

    #[test]
    fn line_chunking_handles_unicode() {
        let src = "héllo wörld\n".repeat(100);
        let chunks = chunk_by_lines(&src, ChunkBudget { max_bytes: 100, min_bytes: 10 });
        assert!(chunks.len() >= 10);
        for c in &chunks {
            assert!(src.is_char_boundary(c.start_byte) && src.is_char_boundary(c.end_byte));
        }
    }
}
