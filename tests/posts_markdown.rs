use my_blog_api::posts::markdown::render_and_sanitize;

#[test]
fn renders_standard_markdown_elements() {
    let md = "# Header 1\n\n## Header 2\n\nParagraph with **bold** and *italic* text.\n\n- item 1\n- item 2\n\n```rust\nfn main() {}\n```";
    let html = render_and_sanitize(md);

    assert!(html.contains("<h1>Header 1</h1>"));
    assert!(html.contains("<h2>Header 2</h2>"));
    assert!(html.contains("<strong>bold</strong>"));
    assert!(html.contains("<em>italic</em>"));
    assert!(html.contains("<li>item 1</li>"));
    assert!(html.contains("<code"));
}

#[test]
fn renders_tables_correctly() {
    let md = "| Col 1 | Col 2 |\n|---|---|\n| Val 1 | Val 2 |";
    let html = render_and_sanitize(md);

    assert!(html.contains("<table>"));
    assert!(html.contains("<thead>"));
    assert!(html.contains("<tbody>"));
    assert!(html.contains("<th>Col 1</th>"));
    assert!(html.contains("<td>Val 1</td>"));
}

#[test]
fn enforces_link_security_attributes() {
    let md = "[External](https://example.com/page)";
    let html = render_and_sanitize(md);

    assert!(html.contains("href=\"https://example.com/page\""));
    assert!(html.contains("rel=\"noopener noreferrer\""));
}

#[test]
fn strips_script_tags_and_inline_content() {
    let md = "Normal text <script>alert('xss')</script> and more text";
    let html = render_and_sanitize(md);

    assert!(!html.contains("<script>"));
    assert!(!html.contains("alert('xss')"));
    assert!(html.contains("Normal text"));
    assert!(html.contains("and more text"));
}

#[test]
fn strips_harmful_html_tags() {
    let dangerous = "Text <iframe src=\"https://evil.com\"></iframe><object data=\"evil.swf\"></object><embed src=\"evil.swf\"><style>body{display:none}</style>";
    let html = render_and_sanitize(dangerous);

    assert!(!html.contains("<iframe"));
    assert!(!html.contains("<object"));
    assert!(!html.contains("<embed"));
    assert!(!html.contains("<style"));
}

#[test]
fn strips_event_handlers_and_javascript_uris() {
    let md = "<img src=\"https://example.com/cat.jpg\" onerror=\"alert(1)\" onload=\"alert(2)\" />\n\n[Attack](javascript:alert(1))";
    let html = render_and_sanitize(md);

    assert!(!html.contains("onerror"));
    assert!(!html.contains("onload"));
    assert!(!html.contains("javascript:"));
}
