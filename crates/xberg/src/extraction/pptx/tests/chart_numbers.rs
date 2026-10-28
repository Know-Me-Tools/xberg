use super::*;

#[test]
fn should_round_chart_values_through_the_pptx_container_gh2298() {
    let slide = r#"<p:sld xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main"
        xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main"
        xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships">
        <p:cSld><p:spTree><p:graphicFrame>
            <a:graphic><a:graphicData uri="http://schemas.openxmlformats.org/drawingml/2006/chart">
                <c:chart xmlns:c="http://schemas.openxmlformats.org/drawingml/2006/chart" r:id="chart"/>
            </a:graphicData></a:graphic>
        </p:graphicFrame></p:spTree></p:cSld></p:sld>"#;
    let relationships = r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
        <Relationship Id="chart" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/chart" Target="../charts/chart1.xml"/>
        </Relationships>"#;
    let chart = br#"<c:chartSpace xmlns:c="http://schemas.openxmlformats.org/drawingml/2006/chart">
        <c:chart><c:plotArea><c:barChart><c:ser>
            <c:val><c:numRef><c:numCache>
                <c:pt idx="0"><c:v>1.1000000000000001</c:v></c:pt>
                <c:pt idx="1"><c:v>42</c:v></c:pt>
            </c:numCache></c:numRef></c:val>
        </c:ser></c:barChart></c:plotArea></c:chart></c:chartSpace>"#;
    let bytes = build_single_slide_pptx(slide, Some(relationships), &[("ppt/charts/chart1.xml", chart)]);
    let result = extract_pptx_from_bytes(&bytes, &PptxExtractionOptions::default(), &mut Vec::new()).unwrap();
    assert_eq!(result.slide_count, 1);
    assert!(result.content.contains("1.1, 42"), "{}", result.content);
    assert!(!result.content.contains("1.1000000000000001"), "{}", result.content);
}
