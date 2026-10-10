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
            "th", "td", "hr", "video", "audio", "source",
        ])
        .add_tag_attributes("code", &["class"])
        .add_tag_attributes("pre", &["class"])
        .add_tag_attributes("video", &["src", "controls", "preload", "poster", "loop", "muted", "width", "height"])
        .add_tag_attributes("audio", &["src", "controls", "preload", "loop", "muted"])
        .add_tag_attributes("source", &["src", "type"])
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

    #[test]
    fn markdown_preserves_safe_video_and_audio_elements() {
        let md = "<video controls src=\"https://example.com/clip.mp4\"></video>\n<audio controls src=\"https://example.com/sound.mp3\"></audio>";
        let html = render_and_sanitize(md);
        assert!(html.contains("<video controls=\"\" src=\"https://example.com/clip.mp4\"></video>") || html.contains("<video src=\"https://example.com/clip.mp4\" controls>"));
        assert!(html.contains("<audio controls=\"\" src=\"https://example.com/sound.mp3\"></audio>") || html.contains("<audio src=\"https://example.com/sound.mp3\" controls>"));
    }

    #[test]
    fn markdown_preserves_code_and_pre_class_attributes() {
        let md = "```mermaid\ngraph TD\n  A --> B\n```";
        let html = render_and_sanitize(md);
        assert!(html.contains("class=\"language-mermaid\""));
    }
}
