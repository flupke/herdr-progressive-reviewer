use std::{
    collections::BTreeMap,
    sync::{
        Arc, Condvar, Mutex,
        atomic::{AtomicUsize, Ordering},
        mpsc,
    },
    time::Duration,
};

use review_explore::Significance;
use serde_json::Value;

use super::{
    Candidate, MAX_RATE_LIMIT_RETRIES, ProviderRequestError, RATE_LIMIT_BACKOFF_BASE,
    RATE_LIMIT_BACKOFF_CAP, classify_with, optimized, rate_limit_backoff, retry_rate_limited,
    retryable_status,
};

fn prepared(index: usize) -> optimized::Prepared {
    optimized::Prepared {
        candidate: Candidate {
            id: format!("window-{index}"),
            units: Vec::new(),
            state: Value::Null,
            references: Vec::new(),
            omissions: Vec::new(),
        },
        body: Value::Null,
        estimated_tokens: 1,
        oversized: false,
    }
}

fn result(prepared: &optimized::Prepared) -> review_explore::SignificanceResult {
    prepared
        .candidate
        .result(Significance::Significant, None, BTreeMap::new(), None, None)
}

#[test]
fn classification_uses_at_most_32_workers_and_records_every_window() {
    let (started, observed) = mpsc::channel();
    let gate = Arc::new((Mutex::new(false), Condvar::new()));
    let in_flight = Arc::new(AtomicUsize::new(0));
    let gate_for_workers = Arc::clone(&gate);
    let in_flight_for_workers = Arc::clone(&in_flight);
    let handle = std::thread::spawn(move || {
        let mut recorded = Vec::new();
        let finished = classify_with(
            (0..64).map(prepared).collect(),
            |result| {
                recorded.push(result.id);
                true
            },
            |window| {
                in_flight_for_workers.fetch_add(1, Ordering::SeqCst);
                started.send(()).unwrap();
                let (lock, released) = &*gate_for_workers;
                let mut open = lock.lock().unwrap();
                while !*open {
                    open = released.wait(open).unwrap();
                }
                in_flight_for_workers.fetch_sub(1, Ordering::SeqCst);
                result(window)
            },
        );
        (finished, recorded)
    });
    for _ in 0..32 {
        observed.recv_timeout(Duration::from_secs(2)).unwrap();
    }
    assert_eq!(in_flight.load(Ordering::SeqCst), 32);
    assert!(observed.recv_timeout(Duration::from_millis(30)).is_err());
    let (lock, released) = &*gate;
    *lock.lock().unwrap() = true;
    released.notify_all();
    let (finished, recorded) = handle.join().unwrap();
    assert!(finished);
    assert_eq!(recorded.len(), 64);
    recorded.iter().enumerate().for_each(|(index, id)| {
        assert!(recorded[..index].iter().all(|earlier| earlier != id));
    });
}

#[test]
fn completed_results_are_recorded_while_other_requests_are_in_flight() {
    let gate = Arc::new((Mutex::new(false), Condvar::new()));
    let gate_for_workers = Arc::clone(&gate);
    let (recorded, observed) = mpsc::channel();
    let handle = std::thread::spawn(move || {
        classify_with(
            vec![prepared(0), prepared(1)],
            |result| {
                recorded.send(result.id).unwrap();
                true
            },
            |window| {
                if window.candidate.id == "window-0" {
                    let (lock, released) = &*gate_for_workers;
                    let mut open = lock.lock().unwrap();
                    while !*open {
                        open = released.wait(open).unwrap();
                    }
                }
                result(window)
            },
        )
    });
    assert_eq!(
        observed.recv_timeout(Duration::from_secs(2)).unwrap(),
        "window-1"
    );
    let (lock, released) = &*gate;
    *lock.lock().unwrap() = true;
    released.notify_all();
    assert_eq!(
        observed.recv_timeout(Duration::from_secs(2)).unwrap(),
        "window-0"
    );
    assert!(handle.join().unwrap());
}

#[test]
fn obsolete_attempt_stops_dispatch_after_the_first_rejected_result() {
    let started = AtomicUsize::new(0);
    let mut recorded = 0;
    let finished = classify_with(
        (0..64).map(prepared).collect(),
        |_| {
            recorded += 1;
            false
        },
        |window| {
            started.fetch_add(1, Ordering::SeqCst);
            result(window)
        },
    );
    assert!(!finished);
    assert_eq!(recorded, 1);
    assert!(started.load(Ordering::SeqCst) <= 32);
}

#[test]
fn provider_rate_limit_statuses_use_capped_exponential_backoff() {
    assert!(retryable_status(reqwest::StatusCode::TOO_MANY_REQUESTS));
    assert!(retryable_status(
        reqwest::StatusCode::from_u16(529).unwrap()
    ));
    assert!(!retryable_status(reqwest::StatusCode::SERVICE_UNAVAILABLE));

    assert_eq!(rate_limit_backoff(0), Duration::from_millis(250));
    assert_eq!(rate_limit_backoff(1), Duration::from_millis(500));
    assert_eq!(rate_limit_backoff(2), Duration::from_secs(1));
    assert_eq!(rate_limit_backoff(3), RATE_LIMIT_BACKOFF_CAP);
    assert_eq!(rate_limit_backoff(u32::MAX), RATE_LIMIT_BACKOFF_CAP);
    assert_eq!(RATE_LIMIT_BACKOFF_BASE, Duration::from_millis(250));
}

#[test]
fn rate_limited_requests_retry_then_return_the_successful_result() {
    let mut attempts = 0;
    let mut waits = Vec::new();
    let result = retry_rate_limited(
        || {
            attempts += 1;
            if attempts <= 2 {
                Err(ProviderRequestError::RateLimited(429))
            } else {
                Ok("classified")
            }
        },
        |delay| waits.push(delay),
    );

    assert_eq!(result.unwrap(), "classified");
    assert_eq!(attempts, 3);
    assert_eq!(
        waits,
        [Duration::from_millis(250), Duration::from_millis(500)]
    );
}

#[test]
fn retries_stop_after_the_bound_and_other_errors_are_not_retried() {
    let mut attempts = 0;
    let mut waits = Vec::new();
    let rate_limited = retry_rate_limited::<()>(
        || {
            attempts += 1;
            Err(ProviderRequestError::RateLimited(529))
        },
        |delay| waits.push(delay),
    );
    let error = rate_limited.unwrap_err();
    assert!(error.contains("529"));
    assert_eq!(attempts, MAX_RATE_LIMIT_RETRIES as usize + 1);
    assert_eq!(waits.len(), MAX_RATE_LIMIT_RETRIES as usize);

    attempts = 0;
    waits.clear();
    let permanent = retry_rate_limited::<()>(
        || {
            attempts += 1;
            Err(ProviderRequestError::Other("Provider HTTP 401".into()))
        },
        |delay| waits.push(delay),
    );
    assert_eq!(permanent.unwrap_err(), "Provider HTTP 401");
    assert_eq!(attempts, 1);
    assert!(waits.is_empty());
}
