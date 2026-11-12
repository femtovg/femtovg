//! Loading an SVG file into a usvg tree, and printing the tree.

use std::{borrow::Cow, collections::HashMap};

/// Reads and parses `path`, dropping invalid filter references first; returns the tree and how many were dropped.
pub fn load(path: &str, options: &usvg::Options) -> Result<(usvg::Tree, usize), String> {
    let text = std::fs::read_to_string(path).map_err(|e| format!("{path}: {e}"))?;
    let (text, dropped) = drop_invalid_filter_references(&text);
    let tree = usvg::Tree::from_str(&text, options).map_err(|e| format!("{path}: {e}"))?;
    Ok((tree, dropped))
}

/// Filter Effects 1, section 5: a `filter` that references something other than a `<filter>` element ignores the
/// whole chain and draws the element unfiltered, as Chromium and Firefox do; usvg keeps SVG 1.1's rule and drops the
/// element instead. So every `filter` attribute whose `url()` targets are not all `<filter>` elements is spliced out
/// of the text before usvg sees it. Filter functions (`blur(2px)`) are not references and never invalidate a chain.
fn drop_invalid_filter_references(svg: &str) -> (Cow<'_, str>, usize) {
    let Ok(doc) = usvg::roxmltree::Document::parse(svg) else {
        // usvg reports the parse error.
        return (Cow::Borrowed(svg), 0);
    };
    let tag_by_id: HashMap<&str, &str> = doc
        .descendants()
        .filter(|n| n.is_element())
        .filter_map(|n| n.attribute("id").map(|id| (id, n.tag_name().name())))
        .collect();
    let mut ranges: Vec<std::ops::Range<usize>> = Vec::new();
    for node in doc.descendants().filter(|n| n.is_element()) {
        let Some(attr) = node
            .attributes()
            .find(|a| a.name() == "filter" && a.namespace().is_none())
        else {
            continue;
        };
        let mut invalid = false;
        let mut rest = attr.value();
        while let Some(start) = rest.find("url(") {
            let after = &rest[start + 4..];
            let Some(end) = after.find(')') else { break };
            let target = after[..end].trim().trim_matches(|c| c == '"' || c == '\'');
            invalid |= match target.strip_prefix('#') {
                Some(id) => tag_by_id.get(id) != Some(&"filter"),
                // An external reference is no filter element of this document.
                None => true,
            };
            rest = &after[end + 1..];
        }
        if invalid {
            ranges.push(attr.range());
        }
    }
    if ranges.is_empty() {
        return (Cow::Borrowed(svg), 0);
    }
    let dropped = ranges.len();
    ranges.sort_by_key(|r| r.start);
    let mut out = String::with_capacity(svg.len());
    let mut at = 0;
    for r in ranges {
        // Take the whitespace before `filter="..."` with it.
        let mut start = r.start;
        while start > at && svg.as_bytes()[start - 1].is_ascii_whitespace() {
            start -= 1;
        }
        out.push_str(&svg[at..start]);
        at = r.end;
    }
    out.push_str(&svg[at..]);
    (Cow::Owned(out), dropped)
}

/// Prints the tree's nodes to stderr, one per line, indented by depth.
pub fn dump(children: &[usvg::Node], depth: usize) {
    let indent = depth * 2;
    for node in children {
        match node {
            usvg::Node::Group(g) => {
                eprintln!(
                    "{:indent$}Group mask={:?} clip={:?} blend={:?} filters={} opacity={} transform={:?}",
                    "",
                    g.mask().map(|m| format!("{:?} rect={:?}", m.kind(), m.rect())),
                    g.clip_path().map(|c| format!(
                        "transform={:?} children={}",
                        c.transform(),
                        c.root().children().len()
                    )),
                    g.blend_mode(),
                    g.filters().len(),
                    g.opacity().get(),
                    g.transform(),
                );
                dump(g.children(), depth + 1);
            }
            usvg::Node::Path(p) => {
                let b = p.abs_bounding_box();
                eprintln!(
                    "{:indent$}Path bbox=({:.0},{:.0} {:.0}x{:.0}) fill={} stroke={}",
                    "",
                    b.x(),
                    b.y(),
                    b.width(),
                    b.height(),
                    p.fill().is_some(),
                    p.stroke().is_some(),
                );
            }
            usvg::Node::Text(_) => eprintln!("{:indent$}Text", ""),
            usvg::Node::Image(i) => eprintln!("{:indent$}Image size={:?}", "", i.size()),
        }
    }
}
