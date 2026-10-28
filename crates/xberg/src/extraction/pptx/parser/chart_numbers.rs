use roxmltree::Node;

use super::CHART_NAMESPACE;

pub(super) fn value_text(node: Node<'_, '_>) -> Option<String> {
    let text = node.text()?.trim();
    if text.is_empty() {
        return None;
    }
    if !is_numeric_value(node) {
        return Some(text.to_string());
    }
    let Some(value) = text.parse::<f64>().ok().filter(|value| value.is_finite()) else {
        return Some(text.to_string());
    };
    // ~keep Office displays 15 significant digits, while numeric caches retain binary noise past them (#2298).
    let rounded = format!("{value:.14e}")
        .parse::<f64>()
        .ok()
        .filter(|value| value.is_finite());
    Some(rounded.map_or_else(|| text.to_string(), |value| value.to_string()))
}

fn is_numeric_value(node: Node<'_, '_>) -> bool {
    let mut numeric = false;
    for ancestor in node.ancestors() {
        if ancestor.tag_name().namespace() != Some(CHART_NAMESPACE) {
            continue;
        }
        match ancestor.tag_name().name() {
            "cat" | "tx" => return false,
            "numCache" | "numLit" => numeric = true,
            _ => {}
        }
    }
    numeric
}
