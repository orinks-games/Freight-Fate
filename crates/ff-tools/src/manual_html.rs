//! A minimal Markdown to HTML renderer for the player manual.
//!
//! The manual (`docs/user-manual.md`) uses a small, fixed subset of
//! Markdown: ATX headings, ordered and unordered lists, pipe tables, and
//! inline bold, code and links. This renders exactly that subset to clean,
//! accessible HTML5 (semantic headings, a real `<table>` with `<th>`
//! headers) so a screen reader user gets a properly structured document in
//! a browser. It is not a general Markdown engine.

use regex::{Captures, Regex};

use crate::util::py_strip;

fn escape(text: &str, quote: bool) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' if quote => out.push_str("&quot;"),
            '\'' if quote => out.push_str("&#x27;"),
            c => out.push(c),
        }
    }
    out
}

struct Patterns {
    heading: Regex,
    ordered: Regex,
    unordered: Regex,
    table_sep: Regex,
    link: Regex,
    code: Regex,
    bold: Regex,
}

impl Patterns {
    fn new() -> Self {
        let re = |p: &str| Regex::new(p).expect("valid regex");
        Self {
            heading: re(r"^(#{1,6})\s+(.*)$"),
            ordered: re(r"^\d+\.\s+(.*)$"),
            unordered: re(r"^-\s+(.*)$"),
            table_sep: re(r"^\|?[\s:|-]*-[\s:|-]*\|?\s*$"),
            link: re(r"\[([^\]]+)\]\(([^)]+)\)"),
            code: re(r"`([^`]+)`"),
            bold: re(r"\*\*([^*]+)\*\*"),
        }
    }

    /// Escape HTML, then apply inline code, bold, and links.
    fn inline(&self, text: &str) -> String {
        let text = escape(text, false);
        let text = self.code.replace_all(&text, "<code>$1</code>");
        let text = self.bold.replace_all(&text, "<strong>$1</strong>");
        self.link
            .replace_all(&text, |c: &Captures| {
                format!("<a href=\"{}\">{}</a>", escape(&c[2], true), &c[1])
            })
            .into_owned()
    }
}

fn split_row(row: &str) -> Vec<String> {
    let mut row = py_strip(row);
    if let Some(rest) = row.strip_prefix('|') {
        row = rest;
    }
    if let Some(rest) = row.strip_suffix('|') {
        row = rest;
    }
    row.split('|').map(|c| py_strip(c).to_string()).collect()
}

/// Render the Markdown body (no surrounding HTML document).
pub fn render_body(md_text: &str) -> String {
    let p = Patterns::new();
    let lines: Vec<&str> = md_text.split('\n').collect();
    let mut out: Vec<String> = Vec::new();
    let mut para: Vec<String> = Vec::new();
    let flush = |para: &mut Vec<String>, out: &mut Vec<String>| {
        if !para.is_empty() {
            out.push(format!("<p>{}</p>", p.inline(&para.join(" "))));
            para.clear();
        }
    };
    let is_table_start = |i: usize| {
        py_strip(lines[i]).starts_with('|')
            && i + 1 < lines.len()
            && p.table_sep.is_match(py_strip(lines[i + 1]))
    };
    let mut i = 0;
    while i < lines.len() {
        let stripped = py_strip(lines[i]);
        if let Some(h) = p.heading.captures(stripped) {
            flush(&mut para, &mut out);
            let level = h[1].len();
            out.push(format!("<h{level}>{}</h{level}>", p.inline(&h[2])));
            i += 1;
        } else if is_table_start(i) {
            flush(&mut para, &mut out);
            let header = split_row(lines[i]);
            i += 2; // header row plus the separator row
            let mut body: Vec<Vec<String>> = Vec::new();
            while i < lines.len() && py_strip(lines[i]).starts_with('|') {
                body.push(split_row(lines[i]));
                i += 1;
            }
            let mut table = vec!["<table>".to_string(), "<thead><tr>".to_string()];
            table.extend(header.iter().map(|c| format!("<th>{}</th>", p.inline(c))));
            table.push("</tr></thead>".into());
            table.push("<tbody>".into());
            for row in body {
                let cells: String = row
                    .iter()
                    .map(|c| format!("<td>{}</td>", p.inline(c)))
                    .collect();
                table.push(format!("<tr>{cells}</tr>"));
            }
            table.push("</tbody>".into());
            table.push("</table>".into());
            out.push(table.join("\n"));
        } else if p.unordered.is_match(stripped) || p.ordered.is_match(stripped) {
            flush(&mut para, &mut out);
            let ordered = !p.unordered.is_match(stripped);
            let (tag, pattern) = if ordered {
                ("ol", &p.ordered)
            } else {
                ("ul", &p.unordered)
            };
            let mut items = Vec::new();
            while i < lines.len() {
                let Some(m) = pattern.captures(py_strip(lines[i])) else {
                    break;
                };
                items.push(format!("<li>{}</li>", p.inline(&m[1])));
                i += 1;
            }
            out.push(format!("<{tag}>\n{}\n</{tag}>", items.join("\n")));
        } else if stripped.is_empty() {
            flush(&mut para, &mut out);
            i += 1;
        } else {
            para.push(stripped.to_string());
            i += 1;
        }
    }
    flush(&mut para, &mut out);
    out.join("\n")
}

const STYLE: &str = "body { font-family: system-ui, -apple-system, sans-serif; line-height: 1.5;
       max-width: 52rem; margin: 2rem auto; padding: 0 1rem; }
h1, h2, h3 { line-height: 1.25; }
table { border-collapse: collapse; width: 100%; margin: 1rem 0; }
th, td { border: 1px solid #999; padding: 0.4rem 0.6rem; text-align: left;
         vertical-align: top; }
th { background: #f0f0f0; }
code { background: #f4f4f4; padding: 0.1rem 0.3rem; border-radius: 3px; }";

/// Render the manual Markdown to a complete, accessible HTML5 document.
pub fn markdown_to_html(md_text: &str, title: &str) -> String {
    format!(
        "<!DOCTYPE html>\n<html lang=\"en\">\n<head>\n<meta charset=\"utf-8\">\n\
         <meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\n\
         <title>{}</title>\n<style>\n{STYLE}\n</style>\n</head>\n<body>\n{}\n</body>\n</html>\n",
        escape(title, true),
        render_body(md_text)
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renders_the_manual_subset() {
        let md = "# Title\n\nSome **bold** and `code` and [a link](x.html?a=1&b=\"2\").\nmore\n\n\
                  - one\n- two\n\n1. first\n2. second\n\n| Key | Does |\n| --- | --- |\n| F1 | Help <now> |\n";
        assert_eq!(
            render_body(md),
            "<h1>Title</h1>\n<p>Some <strong>bold</strong> and <code>code</code> and \
             <a href=\"x.html?a=1&amp;amp;b=&quot;2&quot;\">a link</a>. more</p>\n\
             <ul>\n<li>one</li>\n<li>two</li>\n</ul>\n<ol>\n<li>first</li>\n<li>second</li>\n</ol>\n\
             <table>\n<thead><tr>\n<th>Key</th>\n<th>Does</th>\n</tr></thead>\n<tbody>\n\
             <tr><td>F1</td><td>Help &lt;now&gt;</td></tr>\n</tbody>\n</table>"
        );
    }
}
