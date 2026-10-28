//! Manual native ownership qualification with byte-verified local assets. ~keep

#![cfg(all(feature = "build-tesseract", not(target_arch = "wasm32")))]
#![allow(clippy::print_stderr)] // ~keep Explicit native qualification prints observed counts for its isolated-process receipt.

use std::path::{Path, PathBuf};

use image::{ImageFormat, RgbImage};
use xberg_tesseract::{TessPageSegMode, TesseractAPI};

const DETECTOR_ENGINES: usize = 2;
const CALLS_PER_ENGINE: usize = 3;

fn required_path(name: &str) -> PathBuf {
    PathBuf::from(std::env::var_os(name).unwrap_or_else(|| panic!("{name} must name a verified local prerequisite")))
}

fn read_png(path: &Path) -> RgbImage {
    let bytes = std::fs::read(path).expect("the verified PNG fixture must exist");
    let image = image::load_from_memory_with_format(&bytes, ImageFormat::Png)
        .expect("fixture must be PNG; convert the pinned Chinese JPEG without changing its pixels")
        .into_rgb8();
    assert!(
        image.width() > 0 && image.height() > 0,
        "the fixture must contain actual pixels"
    );
    image
}

fn set_image(api: &TesseractAPI, image: &RgbImage) {
    let width = i32::try_from(image.width()).expect("bounded fixture width");
    let height = i32::try_from(image.height()).expect("bounded fixture height");
    let stride = width.checked_mul(3).expect("bounded RGB stride");
    api.set_image(image.as_raw(), width, height, 3, stride)
        .expect("attach the owned RGB fixture");
    api.set_source_resolution(300)
        .expect("set the qualification input resolution");
}

#[test]
#[ignore = "requires verified local osd/eng packs and PNG corpus fixtures; invoke explicitly in an isolated process"]
fn should_keep_osd_script_names_and_a_separate_recognizer_alive_gh2300() {
    let tessdata = required_path("XBERG_OSD_TESSDATA");
    for language in ["osd", "eng"] {
        let path = tessdata.join(format!("{language}.traineddata"));
        assert!(path.is_file(), "the verified {language}.traineddata pack must exist");
        assert!(
            path.metadata().unwrap().len() > 0,
            "the {language} pack must contain actual bytes"
        );
    }
    let chinese = read_png(&required_path("XBERG_OSD_FIXTURE"));
    let english_path = std::env::var_os("XBERG_OSD_RECOGNIZER_FIXTURE")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../test_documents/images/test_hello_world.png")
        });
    let english = read_png(&english_path);
    let expected_calls = DETECTOR_ENGINES * CALLS_PER_ENGINE;
    eprintln!("OSD_CALLS expected={expected_calls} fresh_engines={DETECTOR_ENGINES}");

    let recognizer = TesseractAPI::new().expect("create a separate recognition engine");
    recognizer
        .init(&tessdata, "eng")
        .expect("initialize the configured English recognizer");
    recognizer.set_page_seg_mode(TessPageSegMode::PSM_AUTO).unwrap();
    set_image(&recognizer, &english);
    recognizer.recognize().expect("run actual baseline recognition");
    let baseline = recognizer.get_utf8_text().expect("read baseline recognition");
    assert!(
        baseline.to_lowercase().contains("hello"),
        "the independent recognizer must read its known English fixture"
    );

    let mut actual_calls = 0;
    for engine in 0..DETECTOR_ENGINES {
        let detector = TesseractAPI::new().expect("create a fresh OSD engine");
        detector.init(&tessdata, "osd").expect("initialize the local OSD model");
        detector.set_page_seg_mode(TessPageSegMode::PSM_OSD_ONLY).unwrap();
        set_image(&detector, &chinese);
        for call in 0..CALLS_PER_ENGINE {
            let (angle, orientation_confidence, script, script_confidence) =
                detector.detect_os().expect("native OSD must succeed");
            actual_calls += 1;
            assert_eq!(script, "Han", "actual Chinese script observation is required");
            assert!(orientation_confidence.is_finite() && script_confidence.is_finite());
            eprintln!(
                "OSD_OBSERVED engine={engine} call={call} angle={angle} script={script} script_confidence={script_confidence}"
            );
        }
        drop(detector);
        recognizer
            .recognize()
            .expect("the separate recognition engine must survive detector drop");
        assert_eq!(
            recognizer.get_utf8_text().unwrap(),
            baseline,
            "OSD must not corrupt or change the configured recognizer"
        );
    }
    assert_eq!(actual_calls, expected_calls);
    drop(recognizer);
    eprintln!("OSD_CALLS expected={expected_calls} actual={actual_calls} recognizer_checks=3");
}
