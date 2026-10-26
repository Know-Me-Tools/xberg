#![cfg(feature = "html")]

mod helpers;

use helpers::extract_bytes_document_blocking;
use xberg::ConversionOptions;
use xberg::core::config::{ContentFilterConfig, ExtractionConfig, OutputFormat};

const SCRIPT_HTML: &[u8] = b"<p>7,2 x 10<sup>9</sup>/l; H<sub>2</sub>O</p>";

fn config(output_format: OutputFormat) -> ExtractionConfig {
    ExtractionConfig {
        output_format,
        escape_markdown: false,
        use_cache: false,
        ..Default::default()
    }
}

fn extract(html: &[u8], config: &ExtractionConfig) -> String {
    extract_bytes_document_blocking(html, "text/html", config)
        .expect("valid HTML extracts through the public bytes API")
        .content
}

#[test]
fn should_keep_html_scripts_in_markdown_without_conversion_options() {
    assert_eq!(
        extract(SCRIPT_HTML, &config(OutputFormat::Markdown)),
        "7,2 x 10^9^/l; H~2~O\n"
    );
}

#[test]
fn should_keep_default_script_markers_with_a_content_filter() {
    let mut config = config(OutputFormat::Markdown);
    config.content_filter = Some(ContentFilterConfig::default());

    assert_eq!(extract(SCRIPT_HTML, &config), "7,2 x 10^9^/l; H~2~O\n");
}

#[test]
fn should_preserve_supplied_empty_script_symbols_as_an_opt_out() {
    let mut config = config(OutputFormat::Markdown);
    config.html_options = Some(ConversionOptions {
        sup_symbol: String::new(),
        sub_symbol: String::new(),
        ..Default::default()
    });

    assert_eq!(extract(SCRIPT_HTML, &config), "7,2 x 109/l; H2O\n");
    config.content_filter = Some(ContentFilterConfig::default());
    assert_eq!(extract(SCRIPT_HTML, &config), "7,2 x 109/l; H2O\n");
}

#[test]
fn should_preserve_custom_script_symbols() {
    let mut config = config(OutputFormat::Markdown);
    config.html_options = Some(ConversionOptions {
        sup_symbol: "<sup>".to_string(),
        sub_symbol: "<sub>".to_string(),
        ..Default::default()
    });

    assert_eq!(
        extract(SCRIPT_HTML, &config),
        "7,2 x 10<sup>9</sup>/l; H<sub>2</sub>O\n"
    );
}

#[test]
fn should_keep_plain_html_output_free_of_markdown_script_markers() {
    assert_eq!(
        extract(SCRIPT_HTML, &config(OutputFormat::Plain)).trim_end(),
        "7,2 x 109/l; H2O"
    );
}

#[test]
fn should_preserve_existing_djot_script_markers() {
    assert_eq!(
        extract(SCRIPT_HTML, &config(OutputFormat::Djot)).trim_end(),
        "7,2 x 10^9^/l; H~2~O"
    );
}
