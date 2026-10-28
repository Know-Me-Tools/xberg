use super::{OcrProcessor, TesseractConfig, dictionary};
use crate::ocr_metadata_keys::OCR_TESSERACT_DICT_INVALID_WORD_RATIO_METADATA_KEY;
use serde_json::{Value, json};
use std::io::Write;
use std::path::Path;
use std::time::Instant;

const WARMUP_CALLS: usize = 2;
const TIMED_CALLS: usize = 6;

struct ArmObservation {
    language: &'static str,
    elapsed_us: Vec<u128>,
    secondary_loads: Vec<usize>,
    dictionary_ratio: f64,
}

fn normalize(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn emit(observation: Value) {
    writeln!(std::io::stderr().lock(), "{observation}").expect("write controlled benchmark evidence");
}

fn run_arm(language: &'static str, image: &[u8], expected: &str, tessdata: &Path) -> ArmObservation {
    let directory = tempfile::tempdir().expect("private benchmark directory");
    let cache = directory.path().join("disabled-cache");
    let processor = OcrProcessor::new(Some(cache.clone())).expect("create persistent processor");
    let config = TesseractConfig {
        language: language.to_owned(),
        tessdata_path: Some(tessdata.to_path_buf()),
        output_format: "text".to_owned(),
        psm: 6,
        oem: 1,
        enable_table_detection: false,
        use_cache: false,
        ..Default::default()
    };
    let mut observation = ArmObservation {
        language,
        elapsed_us: Vec::new(),
        secondary_loads: Vec::new(),
        dictionary_ratio: f64::NAN,
    };
    for index in 0..WARMUP_CALLS + TIMED_CALLS {
        let before = dictionary::secondary_initialization_count();
        let started = Instant::now();
        let result = processor
            .process_image(image, &config)
            .expect("perform actual native OCR");
        let elapsed_us = started.elapsed().as_micros();
        assert!(elapsed_us > 0, "the clock must observe native work");
        let loads = dictionary::secondary_initialization_count() - before;
        assert!(!result.content.trim().is_empty(), "empty OCR cannot qualify");
        assert_eq!(
            normalize(&result.content),
            expected,
            "{language} call {index} changed English text"
        );
        let ratio = result
            .metadata
            .get(OCR_TESSERACT_DICT_INVALID_WORD_RATIO_METADATA_KEY)
            .and_then(Value::as_f64)
            .expect("fixture must produce a genuine dictionary quality ratio");
        assert!(ratio.is_finite() && (0.0..=1.0).contains(&ratio));
        if index == 0 {
            observation.dictionary_ratio = ratio;
        }
        assert_eq!(
            ratio, observation.dictionary_ratio,
            "dictionary quality changed between calls"
        );
        observation.elapsed_us.push(elapsed_us);
        observation.secondary_loads.push(loads);
        emit(
            json!({"language": language, "call": index + 1, "warmup": index < WARMUP_CALLS,
            "elapsed_us": elapsed_us, "secondary_initializations": loads, "dictionary_invalid_word_ratio": ratio}),
        );
    }
    assert!(!cache.exists(), "disabled result caching must not write an artifact");
    observation
}

fn summarize(arm: &ArmObservation) -> Value {
    assert_eq!(arm.elapsed_us.len(), WARMUP_CALLS + TIMED_CALLS);
    assert_eq!(arm.secondary_loads.len(), WARMUP_CALLS + TIMED_CALLS);
    let mut measured = arm.elapsed_us[WARMUP_CALLS..].to_vec();
    measured.sort_unstable();
    let median_us = (measured[TIMED_CALLS / 2 - 1] + measured[TIMED_CALLS / 2]) / 2;
    json!({"language": arm.language, "all_elapsed_us": arm.elapsed_us,
        "timed_elapsed_us": &arm.elapsed_us[WARMUP_CALLS..], "median_us": median_us,
        "secondary_initializations_per_call": arm.secondary_loads,
        "dictionary_invalid_word_ratio": arm.dictionary_ratio})
}

#[test]
#[ignore = "requires qualified eng/khm packs and a controlled English PNG fixture"]
fn should_measure_persistent_secondary_dictionary_ocr_calls_gh2306() {
    let image_path =
        std::env::var("XBERG_DICTIONARY_BENCH_PNG").expect("set the controlled synthetic English PNG path");
    let expected = normalize(
        &std::env::var("XBERG_DICTIONARY_BENCH_EXPECTED_TEXT").expect("set independent English ground truth"),
    );
    assert!(
        expected.is_ascii() && expected.split_whitespace().count() >= 10,
        "provide sufficiently long English prose"
    );
    let tessdata = std::env::var("TESSDATA_PREFIX").expect("set the qualified eng/khm tessdata directory");
    for language in ["eng", "khm"] {
        let path = Path::new(&tessdata).join(format!("{language}.traineddata"));
        assert!(std::fs::metadata(path).expect("required native pack").len() > 0);
    }
    let image = std::fs::read(image_path).expect("read controlled PNG fixture");
    assert!(
        image.starts_with(b"\x89PNG\r\n\x1a\n"),
        "fixture must really be PNG input"
    );
    emit(
        json!({"event": "persistent_dictionary_probe_start", "pid": std::process::id(),
        "fixture_bytes": image.len(), "fixture_blake3": crate::cache::blake3_hash_bytes(&image),
        "warmup_calls_per_arm": WARMUP_CALLS, "timed_calls_per_arm": TIMED_CALLS}),
    );
    let english = run_arm("eng", &image, &expected, Path::new(&tessdata));
    let mixed = run_arm("khm+eng", &image, &expected, Path::new(&tessdata));
    assert_eq!(
        english.dictionary_ratio, mixed.dictionary_ratio,
        "the language arms must retain dictionary quality"
    );
    assert_eq!(english.secondary_loads.iter().sum::<usize>(), 0);
    assert!(
        mixed.secondary_loads.iter().sum::<usize>() > 0,
        "mixed arm must exercise actual secondary initialization"
    );
    emit(
        json!({"event": "persistent_dictionary_probe_complete", "actual_calls": english.elapsed_us.len() + mixed.elapsed_us.len(),
        "arms": [summarize(&english), summarize(&mixed)]}),
    );
}
