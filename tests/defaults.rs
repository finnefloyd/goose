use futures::future::join_all;
use httpmock::{Method::GET, Mock, MockServer};
use serial_test::serial;

mod common;

use goose::logger::GooseLogFormat;
use goose::prelude::*;

// Paths used in load tests performed during these tests.
const INDEX_PATH: &str = "/";
const ABOUT_PATH: &str = "/about.html";

// Indexes to the above paths.
const INDEX_KEY: usize = 0;
const ABOUT_KEY: usize = 1;

// Load test configuration.
const USERS: usize = 3;
const RUN_TIME: usize = 3;
const HATCH_RATE: &str = "10";
const LOG_LEVEL: usize = 0;
const REQUEST_LOG: &str = "request-test.log";
const DEBUG_LOG: &str = "debug-test.log";
const LOG_FORMAT: GooseLogFormat = GooseLogFormat::Raw;
const THROTTLE_REQUESTS: usize = 10;
const EXPECT_WORKERS: usize = 2;

// Can't be tested:
// - GooseDefault::LogFile (logger can only be configured once)
// - GooseDefault::Verbose (logger can only be configured once)
// - GooseDefault::LogLevel (can't validate due to logger limitation)

// Needs followup:
// - GooseDefault::NoMetrics:
//     Gaggles depend on metrics, when disabled load test does not shut down clearly.
// - GooseDefault::StickyFollow
//     Needs more complex tests

// Test task.
pub async fn get_index(user: &mut GooseUser) -> GooseTaskResult {
    let _goose = user.get(INDEX_PATH).await?;
    Ok(())
}

// Test task.
pub async fn get_about(user: &mut GooseUser) -> GooseTaskResult {
    let _goose = user.get(ABOUT_PATH).await?;
    Ok(())
}

// All tests in this file run against common endpoints.
fn setup_mock_server_endpoints(server: &MockServer) -> Vec<Mock> {
    vec![
        // First, set up INDEX_PATH, store in vector at INDEX_KEY.
        server.mock(|when, then| {
            when.method(GET).path(INDEX_PATH);
            then.status(200);
        }),
        // Next, set up ABOUT_PATH, store in vector at ABOUT_KEY.
        server.mock(|when, then| {
            when.method(GET).path(ABOUT_PATH);
            then.status(200);
        }),
    ]
}

// Helper to confirm all variations generate appropriate results.
fn validate_test(
    goose_metrics: &GooseMetrics,
    mock_endpoints: &[Mock],
    requests_files: &[String],
    debug_files: &[String],
) {
    // Confirm that we loaded the mock endpoints. This confirms that we started
    // both users, which also verifies that hatch_rate was properly set.
    assert!(mock_endpoints[INDEX_KEY].hits() > 0);
    assert!(mock_endpoints[ABOUT_KEY].hits() > 0);

    let index_metrics = goose_metrics
        .requests
        .get(&format!("GET {}", INDEX_PATH))
        .unwrap();
    let about_metrics = goose_metrics
        .requests
        .get(&format!("GET {}", ABOUT_PATH))
        .unwrap();

    // Confirm that Goose and the server saw the same number of page loads.
    mock_endpoints[INDEX_KEY].assert_hits(index_metrics.raw_data.counter);
    mock_endpoints[INDEX_KEY].assert_hits(index_metrics.success_count);
    mock_endpoints[ABOUT_KEY].assert_hits(about_metrics.raw_data.counter);
    mock_endpoints[ABOUT_KEY].assert_hits(about_metrics.success_count);
    assert!(index_metrics.fail_count == 0);
    assert!(about_metrics.fail_count == 0);

    // Confirm that we tracked status codes.
    assert!(!index_metrics.status_code_counts.is_empty());
    assert!(!about_metrics.status_code_counts.is_empty());

    // Confirm that we did not track task metrics.
    assert!(goose_metrics.tasks.is_empty());

    // Verify that Goose started the correct number of users.
    assert!(goose_metrics.users == USERS);

    // Verify that the metrics file was created and has the correct number of lines.
    let mut metrics_lines = 0;
    for requests_file in requests_files {
        assert!(std::path::Path::new(requests_file).exists());
        metrics_lines += common::file_length(requests_file);
    }
    assert!(metrics_lines == mock_endpoints[INDEX_KEY].hits() + mock_endpoints[ABOUT_KEY].hits());

    // Verify that the debug file was created and is empty.
    for debug_file in debug_files {
        assert!(std::path::Path::new(debug_file).exists());
        assert!(common::file_length(debug_file) == 0);
    }

    // Requests are made while GooseUsers are hatched, and then for run_time seconds.
    // Verify that the test ran as long as it was supposed to.
    assert!(goose_metrics.duration == RUN_TIME);

    // Be sure there were no more requests made than the throttle should allow.
    // In the case of a gaggle, there's multiple processes running with the same
    // throttle.
    let number_of_processes = requests_files.len();
    assert!(metrics_lines <= (RUN_TIME + 1) * THROTTLE_REQUESTS * number_of_processes);

    // Cleanup from test.
    for file in requests_files {
        common::cleanup_files(vec![file]);
    }
    for file in debug_files {
        common::cleanup_files(vec![file]);
    }
}

#[tokio::test]
// Configure load test with set_default.
async fn test_defaults() {
    // Multiple tests run together, so set a unique name.
    let request_log = "defaults-".to_string() + REQUEST_LOG;
    let debug_log = "defaults-".to_string() + DEBUG_LOG;

    // Be sure there's no files left over from an earlier test.
    common::cleanup_files(vec![&request_log, &debug_log]);

    let server = MockServer::start();

    // Setup the mock endpoints needed for this test.
    let mock_endpoints = setup_mock_server_endpoints(&server);

    let mut config = common::build_configuration(&server, vec![]);

    // Unset options set in common.rs so set_default() is instead used.
    config.users = None;
    config.run_time = "".to_string();
    config.hatch_rate = None;
    let host = std::mem::take(&mut config.host);

    let goose_metrics = crate::GooseAttack::initialize_with_config(config)
        .unwrap()
        .register_taskset(taskset!("Index").register_task(task!(get_index)))
        .register_taskset(taskset!("About").register_task(task!(get_about)))
        // Start at least two users, required to run both TaskSets.
        .set_default(GooseDefault::Host, host.as_str())
        .unwrap()
        .set_default(GooseDefault::Users, USERS)
        .unwrap()
        .set_default(GooseDefault::RunTime, RUN_TIME)
        .unwrap()
        .set_default(GooseDefault::HatchRate, HATCH_RATE)
        .unwrap()
        .set_default(GooseDefault::LogLevel, LOG_LEVEL)
        .unwrap()
        .set_default(GooseDefault::RequestLog, request_log.as_str())
        .unwrap()
        .set_default(GooseDefault::RequestFormat, LOG_FORMAT)
        .unwrap()
        .set_default(GooseDefault::DebugLog, debug_log.as_str())
        .unwrap()
        .set_default(GooseDefault::DebugFormat, LOG_FORMAT)
        .unwrap()
        .set_default(GooseDefault::NoDebugBody, true)
        .unwrap()
        .set_default(GooseDefault::ThrottleRequests, THROTTLE_REQUESTS)
        .unwrap()
        .set_default(GooseDefault::StatusCodes, true)
        .unwrap()
        .set_default(
            GooseDefault::CoordinatedOmissionMitigation,
            GooseCoordinatedOmissionMitigation::Disabled,
        )
        .unwrap()
        .set_default(GooseDefault::RunningMetrics, 0)
        .unwrap()
        .set_default(GooseDefault::NoTaskMetrics, true)
        .unwrap()
        .set_default(GooseDefault::NoResetMetrics, true)
        .unwrap()
        .set_default(GooseDefault::StickyFollow, true)
        .unwrap()
        .execute()
        .await
        .unwrap();

    validate_test(
        &goose_metrics,
        &mock_endpoints,
        &[request_log],
        &[debug_log],
    );

    // Confirm Goose doesn't panic when printing metrics.
    goose_metrics.print();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 8)]
#[cfg_attr(not(feature = "gaggle"), ignore)]
#[serial]
// Configure load test with set_default, run as Gaggle.
async fn test_defaults_gaggle() {
    // Multiple tests run together, so set a unique name.
    let request_log = "gaggle-defaults".to_string() + REQUEST_LOG;
    let debug_log = "gaggle-defaults".to_string() + DEBUG_LOG;

    // Be sure there's no files left over from an earlier test.
    for i in 0..EXPECT_WORKERS {
        let file = request_log.to_string() + &i.to_string();
        common::cleanup_files(vec![&file]);
        let file = debug_log.to_string() + &i.to_string();
        common::cleanup_files(vec![&file]);
    }

    let server = MockServer::start();

    // Setup the mock endpoints needed for this test.
    let mock_endpoints = setup_mock_server_endpoints(&server);

    const HOST: &str = "127.0.0.1";
    const PORT: usize = 9988;

    let mut configuration = common::build_configuration(&server, vec![]);

    // Unset options set in common.rs so set_default() is instead used.
    configuration.users = None;
    configuration.run_time = "".to_string();
    configuration.hatch_rate = None;
    configuration.co_mitigation = None;
    let host = std::mem::take(&mut configuration.host);

    // Launch workers in their own threads, storing the thread handle.
    let mut worker_handles = Vec::new();
    for i in 0..EXPECT_WORKERS {
        let worker_configuration = configuration.clone();
        let worker_request_log = request_log.clone() + &i.to_string();
        let worker_debug_log = debug_log.clone() + &i.to_string();
        worker_handles.push(tokio::spawn(
            crate::GooseAttack::initialize_with_config(worker_configuration)
                .unwrap()
                .register_taskset(taskset!("Index").register_task(task!(get_index)))
                .register_taskset(taskset!("About").register_task(task!(get_about)))
                // Start at least two users, required to run both TaskSets.
                .set_default(GooseDefault::ThrottleRequests, THROTTLE_REQUESTS)
                .unwrap()
                .set_default(GooseDefault::DebugLog, worker_debug_log.as_str())
                .unwrap()
                .set_default(GooseDefault::DebugFormat, LOG_FORMAT)
                .unwrap()
                .set_default(GooseDefault::NoDebugBody, true)
                .unwrap()
                .set_default(GooseDefault::RequestLog, worker_request_log.as_str())
                .unwrap()
                .set_default(GooseDefault::RequestFormat, LOG_FORMAT)
                .unwrap()
                // Worker configuration using defaults instead of run-time options.
                .set_default(GooseDefault::Worker, true)
                .unwrap()
                .set_default(GooseDefault::ManagerHost, HOST)
                .unwrap()
                .set_default(GooseDefault::ManagerPort, PORT)
                .unwrap()
                .execute(),
        ));
    }

    // Start manager instance in current thread and run a distributed load test.
    let goose_metrics = crate::GooseAttack::initialize_with_config(configuration)
        .unwrap()
        // Alter the name of the task set so NoHashCheck is required for load test to run.
        .register_taskset(taskset!("FooIndex").register_task(task!(get_index)))
        .register_taskset(taskset!("About").register_task(task!(get_about)))
        // Start at least two users, required to run both TaskSets.
        .set_default(GooseDefault::Host, host.as_str())
        .unwrap()
        .set_default(GooseDefault::Users, USERS)
        .unwrap()
        .set_default(GooseDefault::RunTime, RUN_TIME)
        .unwrap()
        .set_default(GooseDefault::HatchRate, HATCH_RATE)
        .unwrap()
        .set_default(
            GooseDefault::CoordinatedOmissionMitigation,
            GooseCoordinatedOmissionMitigation::Disabled,
        )
        .unwrap()
        .set_default(GooseDefault::StatusCodes, true)
        .unwrap()
        .set_default(GooseDefault::RunningMetrics, 0)
        .unwrap()
        .set_default(GooseDefault::NoTaskMetrics, true)
        .unwrap()
        .set_default(GooseDefault::StickyFollow, true)
        .unwrap()
        // Manager configuration using defaults instead of run-time options.
        .set_default(GooseDefault::Manager, true)
        .unwrap()
        .set_default(GooseDefault::ExpectWorkers, EXPECT_WORKERS)
        .unwrap()
        .set_default(GooseDefault::NoHashCheck, true)
        .unwrap()
        .set_default(GooseDefault::ManagerBindHost, HOST)
        .unwrap()
        .set_default(GooseDefault::ManagerBindPort, PORT)
        .unwrap()
        .execute()
        .await
        .unwrap();

    // Wait for both worker threads to finish and exit.
    join_all(worker_handles).await;

    let mut request_logs: Vec<String> = vec![];
    let mut debug_logs: Vec<String> = vec![];
    for i in 0..EXPECT_WORKERS {
        let file = request_log.to_string() + &i.to_string();
        request_logs.push(file);
        let file = debug_log.to_string() + &i.to_string();
        debug_logs.push(file);
    }
    validate_test(&goose_metrics, &mock_endpoints, &request_logs, &debug_logs);

    // Confirm Goose doesn't panic when printing metrics.
    goose_metrics.print();
}

#[tokio::test]
// Configure load test with run time options (not with defaults).
async fn test_no_defaults() {
    // Multiple tests run together, so set a unique name.
    let requests_file = "nodefaults-".to_string() + REQUEST_LOG;
    let debug_file = "nodefaults-".to_string() + DEBUG_LOG;

    // Be sure there's no files left over from an earlier test.
    common::cleanup_files(vec![&requests_file, &debug_file]);

    let server = MockServer::start();

    // Setup the mock endpoints needed for this test.
    let mock_endpoints = setup_mock_server_endpoints(&server);

    let config = common::build_configuration(
        &server,
        vec![
            "--users",
            &USERS.to_string(),
            "--hatch-rate",
            &HATCH_RATE.to_string(),
            "--run-time",
            &RUN_TIME.to_string(),
            "--request-log",
            &requests_file,
            "--request-format",
            &format!("{:?}", LOG_FORMAT),
            "--debug-log",
            &debug_file,
            "--debug-format",
            &format!("{:?}", LOG_FORMAT),
            "--no-debug-body",
            "--throttle-requests",
            &THROTTLE_REQUESTS.to_string(),
            "--no-reset-metrics",
            "--no-task-metrics",
            "--status-codes",
            "--running-metrics",
            "30",
            "--sticky-follow",
        ],
    );

    let goose_metrics = crate::GooseAttack::initialize_with_config(config)
        .unwrap()
        .register_taskset(taskset!("Index").register_task(task!(get_index)))
        .register_taskset(taskset!("About").register_task(task!(get_about)))
        .execute()
        .await
        .unwrap();

    validate_test(
        &goose_metrics,
        &mock_endpoints,
        &[requests_file],
        &[debug_file],
    );

    // Confirm Goose doesn't panic when printing metrics.
    goose_metrics.print();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 8)]
#[cfg_attr(not(feature = "gaggle"), ignore)]
#[serial]
// Configure load test with run time options (not with defaults), run as Gaggle.
async fn test_no_defaults_gaggle() {
    let requests_file = "gaggle-nodefaults".to_string() + REQUEST_LOG;
    let debug_file = "gaggle-nodefaults".to_string() + DEBUG_LOG;

    // Be sure there's no files left over from an earlier test.
    for i in 0..EXPECT_WORKERS {
        let file = requests_file.to_string() + &i.to_string();
        common::cleanup_files(vec![&file]);
        let file = debug_file.to_string() + &i.to_string();
        common::cleanup_files(vec![&file]);
    }

    let server = MockServer::start();

    // Setup the mock endpoints needed for this test.
    let mock_endpoints = setup_mock_server_endpoints(&server);

    const HOST: &str = "127.0.0.1";
    const PORT: usize = 9988;

    // Launch workers in their own threads, storing the thread handle.
    let mut worker_handles = Vec::new();
    for i in 0..EXPECT_WORKERS {
        let worker_requests_file = requests_file.to_string() + &i.to_string();
        let worker_debug_file = debug_file.to_string() + &i.to_string();
        let worker_configuration = common::build_configuration(
            &server,
            vec![
                "--worker",
                "--manager-host",
                &HOST.to_string(),
                "--manager-port",
                &PORT.to_string(),
                "--request-log",
                &worker_requests_file,
                "--request-format",
                &format!("{:?}", LOG_FORMAT),
                "--debug-log",
                &worker_debug_file,
                "--debug-format",
                &format!("{:?}", LOG_FORMAT),
                "--no-debug-body",
                "--throttle-requests",
                &THROTTLE_REQUESTS.to_string(),
            ],
        );
        println!("{:#?}", worker_configuration);
        worker_handles.push(tokio::spawn(
            crate::GooseAttack::initialize_with_config(worker_configuration)
                .unwrap()
                .register_taskset(taskset!("Index").register_task(task!(get_index)))
                .register_taskset(taskset!("About").register_task(task!(get_about)))
                .execute(),
        ));
    }

    let manager_configuration = common::build_configuration(
        &server,
        vec![
            "--manager",
            "--expect-workers",
            &EXPECT_WORKERS.to_string(),
            "--manager-bind-host",
            &HOST.to_string(),
            "--manager-bind-port",
            &PORT.to_string(),
            "--users",
            &USERS.to_string(),
            "--hatch-rate",
            &HATCH_RATE.to_string(),
            "--run-time",
            &RUN_TIME.to_string(),
            "--no-reset-metrics",
            "--no-task-metrics",
            "--status-codes",
            "--running-metrics",
            "30",
            "--sticky-follow",
        ],
    );

    let goose_metrics = crate::GooseAttack::initialize_with_config(manager_configuration)
        .unwrap()
        .register_taskset(taskset!("Index").register_task(task!(get_index)))
        .register_taskset(taskset!("About").register_task(task!(get_about)))
        .execute()
        .await
        .unwrap();

    // Wait for both worker threads to finish and exit.
    join_all(worker_handles).await;

    let mut requests_files: Vec<String> = vec![];
    let mut debug_files: Vec<String> = vec![];
    for i in 0..EXPECT_WORKERS {
        let file = requests_file.to_string() + &i.to_string();
        requests_files.push(file);
        let file = debug_file.to_string() + &i.to_string();
        debug_files.push(file);
    }
    validate_test(
        &goose_metrics,
        &mock_endpoints,
        &requests_files,
        &debug_files,
    );

    // Confirm Goose doesn't panic when printing metrics.
    goose_metrics.print();
}

#[tokio::test]
// Configure load test with defaults, disable metrics.
async fn test_defaults_no_metrics() {
    let server = MockServer::start();

    // Setup the mock endpoints needed for this test.
    let mock_endpoints = setup_mock_server_endpoints(&server);

    let mut config = common::build_configuration(&server, vec![]);

    // Unset options set in common.rs so set_default() is instead used.
    config.users = None;
    config.run_time = "".to_string();
    config.hatch_rate = None;

    let goose_metrics = crate::GooseAttack::initialize_with_config(config)
        .unwrap()
        .register_taskset(taskset!("Index").register_task(task!(get_index)))
        .register_taskset(taskset!("About").register_task(task!(get_about)))
        // Start at least two users, required to run both TaskSets.
        .set_default(GooseDefault::Users, USERS)
        .unwrap()
        .set_default(GooseDefault::RunTime, RUN_TIME)
        .unwrap()
        .set_default(GooseDefault::HatchRate, HATCH_RATE)
        .unwrap()
        .set_default(GooseDefault::NoMetrics, true)
        .unwrap()
        .execute()
        .await
        .unwrap();

    // Confirm that we loaded the mock endpoints.
    assert!(mock_endpoints[INDEX_KEY].hits() > 0);
    assert!(mock_endpoints[ABOUT_KEY].hits() > 0);

    // Confirm that we did not track metrics.
    assert!(goose_metrics.requests.is_empty());
    assert!(goose_metrics.tasks.is_empty());
    assert!(goose_metrics.users == USERS);
    assert!(goose_metrics.duration == RUN_TIME);

    // Confirm Goose doesn't panic when printing empty metrics.
    goose_metrics.print();
}
