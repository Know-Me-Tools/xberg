use super::parse_chart_text;

fn chart_text(body: &str) -> Option<String> {
    let xml = format!(
        r#"<c:chartSpace xmlns:c="http://schemas.openxmlformats.org/drawingml/2006/chart"><c:chart>{body}</c:chart></c:chartSpace>"#
    );
    parse_chart_text(xml.as_bytes()).expect("valid chart XML")
}

fn cache(values: &[&str], kind: &str) -> String {
    let points = values
        .iter()
        .enumerate()
        .map(|(index, value)| format!(r#"<c:pt idx="{index}"><c:v>{value}</c:v></c:pt>"#))
        .collect::<String>();
    format!("<c:{kind}>{points}</c:{kind}>")
}

#[test]
fn should_round_numeric_chart_cache_to_office_precision_gh2298() {
    let cache = cache(
        &[
            "1.1000000000000001",
            "42",
            "118.99999999999999",
            "0.30000000000000004",
            "-1.1000000000000001",
        ],
        "numCache",
    );
    assert_eq!(
        chart_text(&format!("<c:ser><c:val><c:numRef>{cache}</c:numRef></c:val></c:ser>")),
        Some("1.1, 42, 119, 0.3, -1.1".to_string())
    );
}

#[test]
fn should_preserve_numeric_looking_category_and_series_labels_gh2298() {
    let categories = cache(&["1.1000000000000001", "00042"], "numCache");
    let series = cache(&["118.99999999999999"], "strCache");
    let values = cache(&["0.30000000000000004"], "strCache");
    assert_eq!(
        chart_text(&format!(
            "<c:ser><c:cat><c:numRef>{categories}</c:numRef></c:cat><c:tx><c:strRef>{series}</c:strRef></c:tx><c:val><c:strRef>{values}</c:strRef></c:val></c:ser>"
        )),
        Some("1.1000000000000001, 00042, 118.99999999999999, 0.30000000000000004".to_string())
    );
}

#[test]
fn should_round_numeric_chart_literals_at_the_significant_digit_boundary_gh2298() {
    let values = cache(
        &["123456789012345.6", "999999999999999.9", "0.00012345678901234567"],
        "numLit",
    );
    assert_eq!(
        chart_text(&format!("<c:ser><c:yVal>{values}</c:yVal></c:ser>")),
        Some("123456789012346, 1000000000000000, 0.000123456789012346".to_string())
    );
}

#[test]
fn should_keep_unrepresentable_and_nonnumeric_chart_values_gh2298() {
    let values = cache(
        &["1.7976931348623157E308", "NaN", "INF", "-INF", "#N/A", "not-a-number"],
        "numCache",
    );
    assert_eq!(
        chart_text(&format!("<c:val><c:numRef>{values}</c:numRef></c:val>")),
        Some("1.7976931348623157E308, NaN, INF, -INF, #N/A, not-a-number".to_string())
    );
}

#[test]
fn should_ignore_empty_chart_values_without_inventing_text_gh2298() {
    let values = cache(&["", "  "], "numCache");
    assert_eq!(
        chart_text(&format!("<c:val><c:numRef>{values}</c:numRef></c:val>")),
        None
    );
}
