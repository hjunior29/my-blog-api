use ammonia::Builder;
use pulldown_cmark::{Options, Parser, html};

pub fn render_and_sanitize(markdown: &str) -> String {
    let mut options = Options::empty();
    options.insert(Options::ENABLE_TABLES);
    options.insert(Options::ENABLE_STRIKETHROUGH);
    options.insert(Options::ENABLE_TASKLISTS);

    let parser = Parser::new_ext(markdown, options);
    let mut raw_html = String::new();
    html::push_html(&mut raw_html, parser);

    let mut cleaner = Builder::new();
    cleaner
        .add_tags(&[
            "h1", "h2", "h3", "h4", "h5", "h6", "del", "ins", "table", "thead", "tbody", "tr",
            "th", "td", "hr",
        ])
        .link_rel(Some("noopener noreferrer"));

    cleaner.clean(&raw_html).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn markdown_renders_valid_elements_and_strips_scripts_and_events() {
        let md =
            "# Title\n\nParagraph with [Link](https://example.com) and <script>alert(1)</script>";
        let html = render_and_sanitize(md);
        assert!(html.contains("<h1>Title</h1>"));
        assert!(html.contains("Paragraph with"));
        assert!(html.contains("rel=\"noopener noreferrer\""));
        assert!(!html.contains("<script>"));
        assert!(!html.contains("alert(1)"));
    }

    #[test]
    fn markdown_strips_onerror_and_javascript_schemes() {
        let md = "<img src=\"x\" onerror=\"alert('xss')\" /> [Click me](javascript:alert(1))";
        let html = render_and_sanitize(md);
        assert!(!html.contains("onerror"));
        assert!(!html.contains("javascript:"));
    }
}
