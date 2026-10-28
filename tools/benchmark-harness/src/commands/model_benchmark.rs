//! `model-benchmark`: detector-only A/B measurements, separate from table recognition.

use benchmark_harness::Result;
use benchmark_harness::model_benchmark::{ModelBenchmarkConfig, print_model_table, run_model_benchmark};
use std::path::PathBuf;

pub(crate) async fn execute(config: ModelBenchmarkConfig, output: PathBuf) -> Result<()> {
    let report = run_model_benchmark(&config).await?;
    report.validate()?;
    if let Some(parent) = output.parent().filter(|parent| !parent.as_os_str().is_empty()) {
        tokio::fs::create_dir_all(parent).await?;
    }
    tokio::fs::write(&output, serde_json::to_vec_pretty(&report)?).await?;
    print_model_table(&report);
    eprintln!("Detector artifact: {}", output.display());
    Ok(())
}

pub(crate) async fn validate_artifact(path: PathBuf) -> Result<()> {
    let bytes = tokio::fs::read(path).await?;
    let report: benchmark_harness::model_benchmark::ModelBenchmarkReport = serde_json::from_slice(&bytes)?;
    report.validate()?;
    print_model_table(&report);
    Ok(())
}
