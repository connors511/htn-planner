//! Synthetic Deferred collector overhead, separate from optional trace capture.
//! Run: `continuation_explanation <new-output-directory> <declared-source> <declared-exe-sha>`.
//! Labels are supplied, unverified provenance; an external runner must hash the actual inputs.
#[path = "continuation_explanation/corpus.rs"]
mod corpus;
use corpus::{Case, Fixture, fixture, validate};
use htn_planner::{
    PlanningResult, SearchExplanation, expand_deferred_explained_with_queries,
    expand_deferred_with_queries,
};
use serde::Serialize;
use std::{env, fs, io::Write, path::Path, time::Instant};
const REPEATS: usize = 5;
const CALLS: usize = 1000;
fn require(test: bool, message: &str) -> Result<(), String> {
    if test { Ok(()) } else { Err(message.into()) }
}
#[derive(Serialize)]
struct Explanation {
    resolution: String,
    selected_empty: Option<htn_planner::Locator>,
    // Debug is an explicitly diagnostic representation, not a frozen wire codec.
    failures_debug: String,
    explored_rejections: u64,
    recovered_rejections: u64,
    omitted_rejections: u64,
    exhausted_queries: u64,
    first_success_cuts: u64,
    truncated: bool,
}
impl From<&SearchExplanation> for Explanation {
    fn from(e: &SearchExplanation) -> Self {
        Self {
            resolution: format!("{:?}", e.resolution),
            selected_empty: e.selected_empty.clone(),
            failures_debug: format!("{:?}", e.failures),
            explored_rejections: e.explored_rejections,
            recovered_rejections: e.recovered_rejections,
            omitted_rejections: e.omitted_rejections,
            exhausted_queries: e.exhausted_queries,
            first_success_cuts: e.first_success_cuts,
            truncated: e.truncated,
        }
    }
}
#[derive(Serialize)]
struct Nanoseconds {
    minimum: u64,
    median: u64,
    p95: u64,
    p99: u64,
    maximum: u64,
}
fn distribution(samples: &[u64]) -> Nanoseconds {
    let mut sorted = samples.to_vec();
    sorted.sort_unstable();
    let rank = |percent: usize| sorted[(sorted.len() * percent).div_ceil(100) - 1];
    Nanoseconds {
        minimum: sorted[0],
        median: rank(50),
        p95: rank(95),
        p99: rank(99),
        maximum: sorted[sorted.len() - 1],
    }
}
#[derive(Serialize)]
struct Repeat {
    ordinal: usize,
    off_ns: Vec<u64>,
    on_ns: Vec<u64>,
    off: Nanoseconds,
    on: Nanoseconds,
}
#[derive(Serialize)]
struct CaseReport {
    name: &'static str,
    supplied_domain: String,
    explanation: Explanation,
    outcome_debug: String,
    work: htn_planner::WorkReport,
    all_result_and_work_pairs_equal: bool,
    repeats: Vec<Repeat>,
}
fn off(f: &Fixture) -> (PlanningResult, u64) {
    let start = Instant::now();
    let result = expand_deferred_with_queries(&f.domain, &f.facts, &f.provider, &f.token);
    let elapsed = u64::try_from(start.elapsed().as_nanos()).unwrap_or(u64::MAX);
    (result, elapsed)
}
fn on(f: &Fixture) -> (PlanningResult, SearchExplanation, u64) {
    let start = Instant::now();
    let result =
        expand_deferred_explained_with_queries(&f.domain, &f.facts, &f.provider, &f.token, None);
    let elapsed = u64::try_from(start.elapsed().as_nanos()).unwrap_or(u64::MAX);
    (result.result, result.explanation, elapsed)
}
fn measure(f: &Fixture) -> Result<CaseReport, String> {
    let mut repeats = Vec::new();
    let mut representative = None;
    for repeat in 0..REPEATS {
        let mut a = Vec::with_capacity(CALLS);
        let mut b = Vec::with_capacity(CALLS);
        for call in 0..CALLS {
            let (plain, explained) = if (repeat + call).is_multiple_of(2) {
                (off(f), on(f))
            } else {
                let explained = on(f);
                (off(f), explained)
            };
            require(
                plain.0 == explained.0,
                "complete result/diagnostic/WorkReport parity failed",
            )?;
            validate(f, &explained.0, &explained.1)?;
            if representative.is_none() {
                representative = Some((
                    Explanation::from(&explained.1),
                    format!("{:?}", explained.0.outcome),
                    explained.0.work.clone(),
                ));
            }
            a.push(plain.1);
            b.push(explained.2);
            // Returned result/explanation payloads drop outside clocks; internal cleanup remains timed.
        }
        repeats.push(Repeat {
            ordinal: repeat,
            off: distribution(&a),
            on: distribution(&b),
            off_ns: a,
            on_ns: b,
        });
    }
    let (explanation, outcome_debug, work) = representative.ok_or("missing samples")?;
    Ok(CaseReport {
        name: f.case.name(),
        supplied_domain: f.text.clone(),
        explanation,
        outcome_debug,
        work,
        all_result_and_work_pairs_equal: true,
        repeats,
    })
}
#[derive(Serialize)]
struct Report {
    format_version: u8,
    status: &'static str,
    declared_source_unverified: String,
    declared_executable_sha_unverified: String,
    profile: &'static str,
    timer_boundary: &'static str,
    repeats: usize,
    calls_per_mode_per_repeat: usize,
    cases: usize,
    timed_expansions: usize,
    exact_pairs: usize,
    trace_capacity: Option<usize>,
    percentile_method: &'static str,
    limitations: [&'static str; 3],
    reports: Vec<CaseReport>,
}
fn write(path: &Path, value: &impl Serialize) -> Result<(), String> {
    let text = ron::ser::to_string(value).map_err(|error| error.to_string())?;
    let mut file = fs::OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(path)
        .map_err(|error| error.to_string())?;
    file.write_all(text.as_bytes())
        .map_err(|error| error.to_string())
}
fn main() -> Result<(), String> {
    let args: Vec<_> = env::args().collect();
    require(
        args.len() == 4,
        "expected new-output-directory declared-source declared-executable-sha",
    )?;
    require(
        args[2].len() <= 256 && args[3].len() <= 256,
        "declared label exceeds 256 bytes",
    )?;
    let output = Path::new(&args[1]);
    fs::create_dir(output).map_err(|error| error.to_string())?;
    write(
        &output.join("started.ron"),
        &"incomplete; no result claim until report.ron exists",
    )?;
    // Compile/admit every supplied source and capture actual tokens before any timer.
    let corpus = Case::ALL
        .into_iter()
        .map(fixture)
        .collect::<Result<Vec<_>, _>>()?;
    for f in &corpus {
        let result = expand_deferred_explained_with_queries(
            &f.domain,
            &f.facts,
            &f.provider,
            &f.token,
            None,
        );
        require(result.trace.is_none(), "trace unexpectedly enabled")?;
        validate(f, &result.result, &result.explanation)?;
    }
    let reports = corpus.iter().map(measure).collect::<Result<Vec<_>, _>>()?;
    write(
        &output.join("report.ron"),
        &Report {
            format_version: 1,
            status: "complete",
            declared_source_unverified: args[2].clone(),
            declared_executable_sha_unverified: args[3].clone(),
            profile: "synthetic Deferred collector OFF vs fixed-four-record ON; optional trace disabled",
            timer_boundary: "expansion call only; parsing/admission/token capture/comparison/reporting/returned-payload disposal outside clocks; internal cleanup included",
            repeats: REPEATS,
            calls_per_mode_per_repeat: CALLS,
            cases: corpus.len(),
            timed_expansions: corpus.len() * REPEATS * CALLS * 2,
            exact_pairs: corpus.len() * REPEATS * CALLS,
            trace_capacity: None,
            percentile_method: "nearest rank per repeat (median is lower central sample)",
            limitations: [
                "Synthetic provider; no production geometry or whole DP boundary claim",
                "No optional-trace overhead, allocator measurement or universal latency claim",
                "External guarded runner must bind actual source/executable/host/build flags; labels are not proof",
            ],
            reports,
        },
    )
}
