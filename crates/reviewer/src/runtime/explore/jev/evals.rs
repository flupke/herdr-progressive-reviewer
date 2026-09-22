//! Opt-in experiments; none of these strategies changes production planning.

mod dataset;
mod metrics;
mod plan;
mod run;
mod sections;
mod study;
mod window;

#[cfg(test)]
mod tests;

#[test]
#[ignore = "paid Jev requests; enable jev-evals, set TYPESAFE_API_KEY, and pass --ignored"]
fn live_compare_strategies() -> eyre::Result<()> {
    run::Experiment::from_env()?.run()
}

#[test]
#[ignore = "offline request export; set JEV_EVAL_OUTPUT and pass --ignored"]
fn export_recursive_requests() -> eyre::Result<()> {
    run::Experiment::from_env()?.export_recursive()
}

#[test]
#[ignore = "offline factorial study export; set JEV_STUDY_CONFIG and pass --ignored"]
fn export_study_requests() -> eyre::Result<()> {
    study::Study::from_env()?.export()
}
