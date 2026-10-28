//! Actual cached-model acceptance for explicit page selection; never skips missing prerequisites.

use std::sync::{Arc, Mutex};

use sha2::{Digest, Sha256};
use tracing::field::{Field, Visit};
use tracing_subscriber::Layer;
use tracing_subscriber::layer::{Context, SubscriberExt};

use super::{GatedPageHandling, LayoutRunOutput, run_layout_for_pdf_pages_with_security_limits_and_pages};
use crate::core::config::{
    AccelerationConfig, ExecutionProviderType, ExtractionConfig, LayoutDetectionConfig, TableModel,
};
use crate::core::layout_pages::LayoutPageSelection;
use crate::extractors::security::SecurityLimits;

#[derive(Clone, Default)]
struct InferenceCapture(Arc<Mutex<Vec<u64>>>);

#[derive(Default)]
struct InferenceEvent {
    detector: bool,
    batch_size: Option<u64>,
}

impl Visit for InferenceEvent {
    fn record_u64(&mut self, field: &Field, value: u64) {
        if field.name() == "batch_size" {
            self.batch_size = Some(value);
        }
    }

    fn record_debug(&mut self, field: &Field, value: &dyn std::fmt::Debug) {
        if field.name() == "message" {
            self.detector = format!("{value:?}") == "Layout engine detect_batch() breakdown";
        }
    }
}

impl<S: tracing::Subscriber> Layer<S> for InferenceCapture {
    fn on_event(&self, event: &tracing::Event<'_>, _context: Context<'_, S>) {
        let mut fields = InferenceEvent::default();
        event.record(&mut fields);
        if fields.detector {
            self.0
                .lock()
                .unwrap()
                .push(fields.batch_size.expect("detector event must report batch size"));
        }
    }
}

fn cpu_layout() -> LayoutDetectionConfig {
    LayoutDetectionConfig {
        table_model: TableModel::Disabled,
        acceleration: Some(AccelerationConfig {
            provider: ExecutionProviderType::Cpu,
            ..Default::default()
        }),
        ..Default::default()
    }
}

fn pinned_memo() -> Vec<u8> {
    let manager = crate::layout::LayoutModelManager::new(None);
    let model = manager
        .verified_model_path("rtdetr")
        .expect("this acceptance test requires the genuine checksum-verified cached RT-DETR model");
    assert_eq!(std::fs::metadata(model).unwrap().len(), 169_089_059);
    let bytes = crate::utils::read_test_fixture("pdf/fake_memo.pdf").expect("required layout-pdf-fast corpus memo");
    assert_eq!(bytes.len(), 13_374);
    assert_eq!(
        hex::encode(Sha256::digest(&bytes)),
        "f54e68281e2018cc3200626aa3cc970c63b4ff2f3afb89a4204207a66f5a2aeb"
    );
    bytes
}

fn infer_with_capture(bytes: &[u8], pages: Vec<u32>) -> (LayoutRunOutput, Vec<u64>) {
    let capture = InferenceCapture::default();
    let subscriber = tracing_subscriber::registry().with(capture.clone());
    // ~keep The synchronous runner stays on this subscriber's thread; spawn_blocking does not inherit it.
    let (output, warning) = tracing::subscriber::with_default(subscriber, || {
        tracing::info!(
            batch_size = 7u64,
            "unrelated event must not count as detector inference"
        );
        run_layout_for_pdf_pages_with_security_limits_and_pages(
            bytes,
            &cpu_layout(),
            1,
            GatedPageHandling::SkipRender,
            &SecurityLimits::default(),
            None,
            Some(&LayoutPageSelection::new(pages)),
        )
        .expect("real selected-page layout inference must succeed")
    });
    assert!(
        warning.is_none(),
        "the corpus memo must render without failure: {warning:?}"
    );
    let batches = capture.0.lock().unwrap().clone();
    (output, batches)
}

fn assert_public_gate(document: &crate::ExtractedDocument, selected: bool) {
    let Some(crate::types::FormatMetadata::Pdf(metadata)) = &document.metadata.format else {
        panic!("the real public route must retain PDF metadata");
    };
    assert_eq!(
        metadata.layout_gated_pages,
        Some(if selected { vec![] } else { vec![1] })
    );
    let reason = if selected {
        "explicit_selection"
    } else {
        "explicit_exclusion"
    };
    assert_eq!(metadata.layout_gate_reasons, Some(vec![reason.to_string()]));
    assert!(
        document
            .processing_warnings
            .iter()
            .all(|warning| warning.source != "layout"),
        "public extraction must not hide layout failure: {:?}",
        document.processing_warnings
    );
    assert!(
        document.content.contains("20,000"),
        "the native memo content must survive"
    );
    assert!(document.content.contains("Mallori"), "the memo signature must survive");
}

#[test]
#[ignore = "requires pinned RT-DETR weights and the corpus memo; qualify with --ignored"]
#[serial_test::serial]
fn should_infer_selected_corpus_page_with_real_model_and_preserve_public_route_gh2255() {
    let bytes = pinned_memo();
    let page_count = xberg_native_pdf::PdfDocument::from_bytes(bytes.clone())
        .unwrap()
        .page_count()
        .unwrap();
    assert_eq!(
        page_count, 1,
        "the bounded layout-pdf-fast memo control is one physical page"
    );
    let (skipped, skipped_batches) = infer_with_capture(&bytes, Vec::new());
    assert!(skipped.data.is_none());
    assert_eq!(
        skipped_batches,
        Vec::<u64>::new(),
        "an excluded page must never invoke the detector"
    );
    let (selected, selected_batches) = infer_with_capture(&bytes, vec![1]);
    assert_eq!(
        selected_batches,
        vec![1],
        "exactly one real physical page must reach model inference"
    );
    let (_, _, _, detections) = selected.data.expect("selected inference must retain actual detections");
    assert_eq!(detections.len(), 1);
    assert!(detections[0].page_width > 1 && detections[0].page_height > 1);
    assert!(
        !detections[0].detections.is_empty(),
        "the real memo must produce nonempty layout detections"
    );

    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let config = ExtractionConfig {
        disable_ocr: true,
        use_cache: true,
        layout: Some(cpu_layout()),
        ..Default::default()
    };
    for (pages, selected) in [(Vec::new(), false), (vec![1], true)] {
        let input =
            crate::ExtractInput::from_bytes(bytes.clone(), "application/pdf", Some("fake_memo.pdf".to_string()));
        let output = runtime
            .block_on(crate::extract_with_layout_pages(input, &config, pages))
            .expect("the real public extraction route must succeed without layout degradation");
        assert_eq!(output.results.len(), 1);
        // ~keep Public-route metadata is separate acceptance; the scoped call counts above belong to the synchronous runner.
        assert_public_gate(&output.results[0], selected);
    }
}
