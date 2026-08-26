use std::future::Future;
use std::path::PathBuf;
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use time::OffsetDateTime;
use tokio::sync::{Mutex as AsyncMutex, Notify, watch};
use tokio::task::JoinHandle;

use crate::application;

use super::list;

const COLLECTION_INTERVAL: Duration = Duration::from_secs(5);

type CollectionFuture = Pin<Box<dyn Future<Output = application::Result<list::Response>> + Send>>;

trait Collector: Send + Sync {
    fn collect(&self, request: list::Request) -> CollectionFuture;
}

trait Clock: Send + Sync {
    fn now(&self) -> OffsetDateTime;
}

struct SystemCollector;

impl Collector for SystemCollector {
    fn collect(&self, request: list::Request) -> CollectionFuture {
        Box::pin(list::execute_with_observer(request, |_| {}))
    }
}

struct SystemClock;

impl Clock for SystemClock {
    fn now(&self) -> OffsetDateTime {
        OffsetDateTime::now_utc()
    }
}

#[derive(Clone)]
pub(crate) struct PrinterMonitor {
    inner: Arc<Inner>,
}

struct Inner {
    config: Option<PathBuf>,
    collector: Arc<dyn Collector>,
    clock: Arc<dyn Clock>,
    snapshots: watch::Sender<Option<Snapshot>>,
    collection: AsyncMutex<()>,
    state: Mutex<State>,
}

struct State {
    subscribers: usize,
    generation: u64,
    refresh: Option<Arc<Notify>>,
    task: Option<JoinHandle<()>>,
}

pub(crate) struct Subscription {
    receiver: watch::Receiver<Option<Snapshot>>,
    retained_snapshot: Option<Snapshot>,
    monitor: PrinterMonitor,
}

impl PrinterMonitor {
    pub(crate) fn new(config: Option<PathBuf>) -> Self {
        Self::with_dependencies(config, Arc::new(SystemCollector), Arc::new(SystemClock))
    }

    fn with_dependencies(
        config: Option<PathBuf>,
        collector: Arc<dyn Collector>,
        clock: Arc<dyn Clock>,
    ) -> Self {
        let (snapshots, _) = watch::channel(None);
        Self {
            inner: Arc::new(Inner {
                config,
                collector,
                clock,
                snapshots,
                collection: AsyncMutex::new(()),
                state: Mutex::new(State {
                    subscribers: 0,
                    generation: 0,
                    refresh: None,
                    task: None,
                }),
            }),
        }
    }

    pub(crate) fn subscribe(&self) -> Subscription {
        let mut receiver = self.inner.snapshots.subscribe();
        let retained_snapshot = receiver.borrow_and_update().clone();
        let mut state = self
            .inner
            .state
            .lock()
            .expect("the printer monitor state mutex should not be poisoned");
        state.subscribers += 1;
        if state.subscribers == 1 {
            state.generation += 1;
            let generation = state.generation;
            let refresh = Arc::new(Notify::new());
            state.refresh = Some(Arc::clone(&refresh));
            let monitor = self.clone();
            state.task = Some(tokio::spawn(async move {
                monitor.run(generation, refresh).await;
            }));
        }
        Subscription {
            receiver,
            retained_snapshot,
            monitor: self.clone(),
        }
    }

    pub(crate) fn request_refresh(&self) {
        let refresh = {
            let state = self
                .inner
                .state
                .lock()
                .expect("the printer monitor state mutex should not be poisoned");
            (state.subscribers != 0)
                .then(|| state.refresh.as_ref().map(Arc::clone))
                .flatten()
        };
        if let Some(refresh) = refresh {
            refresh.notify_one();
        }
    }

    async fn run(self, generation: u64, refresh: Arc<Notify>) {
        let mut forced = true;
        loop {
            if !self.collect_and_publish(generation, forced).await {
                return;
            }
            forced = false;
            tokio::select! {
                _ = tokio::time::sleep(COLLECTION_INTERVAL) => {}
                _ = refresh.notified() => {}
            }
        }
    }

    async fn collect_and_publish(&self, generation: u64, forced: bool) -> bool {
        let _collection = self.inner.collection.lock().await;
        let active = {
            let state = self
                .inner
                .state
                .lock()
                .expect("the printer monitor state mutex should not be poisoned");
            state.subscribers != 0 && state.generation == generation
        };
        if !active {
            return false;
        }
        let response = self
            .inner
            .collector
            .collect(list::Request {
                config: self.inner.config.clone(),
                transport: None,
            })
            .await;
        let snapshot = match response {
            Ok(response) => snapshot_from_response(response, self.inner.clock.now()),
            Err(error) => Snapshot {
                updated_at: self.inner.clock.now(),
                warning: Some(error.to_string()),
                printers: self
                    .inner
                    .snapshots
                    .borrow()
                    .as_ref()
                    .map_or_else(Vec::new, |snapshot| snapshot.printers.clone()),
            },
        };
        let state = self
            .inner
            .state
            .lock()
            .expect("the printer monitor state mutex should not be poisoned");
        if state.subscribers == 0 || state.generation != generation {
            return false;
        }
        let previous = self.inner.snapshots.borrow().clone();
        if should_publish(previous.as_ref(), &snapshot, forced) {
            self.inner.snapshots.send_replace(Some(snapshot));
        }
        true
    }
}

impl Subscription {
    pub(crate) async fn next(&mut self) -> Option<Snapshot> {
        if let Some(snapshot) = self.retained_snapshot.take() {
            return Some(snapshot);
        }
        loop {
            self.receiver.changed().await.ok()?;
            if let Some(snapshot) = self.receiver.borrow_and_update().clone() {
                return Some(snapshot);
            }
        }
    }
}

impl Drop for Subscription {
    fn drop(&mut self) {
        let mut state = self
            .monitor
            .inner
            .state
            .lock()
            .expect("the printer monitor state mutex should not be poisoned");
        state.subscribers -= 1;
        if state.subscribers == 0
            && let Some(task) = state.task.take()
        {
            state.generation += 1;
            state.refresh = None;
            task.abort();
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Snapshot {
    pub(crate) updated_at: OffsetDateTime,
    pub(crate) warning: Option<String>,
    pub(crate) printers: Vec<list::Printer>,
}

pub(crate) async fn collect_once(request: list::Request) -> application::Result<Snapshot> {
    let response = list::execute_with_observer(request, |_| {}).await?;
    Ok(snapshot_from_response(response, OffsetDateTime::now_utc()))
}

fn snapshot_from_response(response: list::Response, updated_at: OffsetDateTime) -> Snapshot {
    Snapshot {
        updated_at,
        warning: None,
        printers: response.printers,
    }
}

fn should_publish(previous: Option<&Snapshot>, next: &Snapshot, forced: bool) -> bool {
    forced
        || previous.is_none()
        || previous.is_some_and(|previous| {
            previous.warning != next.warning || previous.printers != next.printers
        })
}

#[cfg(test)]
mod tests {
    use std::collections::VecDeque;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
    use std::sync::{Arc, Condvar, Mutex};

    use super::*;
    use crate::features::printers::list::{ConnectionFacts, NetworkConnectionFacts};
    use crate::features::printers::{Availability, Transport};

    #[tokio::test]
    async fn one_shot_snapshot_wraps_the_structured_inventory() {
        let response = list::Response {
            config_path: PathBuf::from("/tmp/printers.toml"),
            printers: vec![network_printer("kitchen", Availability::Connected)],
        };
        let now = OffsetDateTime::from_unix_timestamp(1_787_754_730)
            .expect("the fixed test timestamp should be valid");

        let snapshot = snapshot_from_response(response, now);

        assert_eq!(snapshot.updated_at, now);
        assert_eq!(snapshot.warning, None);
        assert_eq!(snapshot.printers[0].name, "kitchen");
    }

    #[tokio::test(start_paused = true)]
    async fn first_subscriber_gets_retained_then_forced_fresh_snapshot() {
        let collector = ScriptedCollector::new([success("old"), success("old")]);
        let monitor = test_monitor(collector.clone());

        let mut first = monitor.subscribe();
        assert_eq!(first.next().await.unwrap().printers[0].name, "old");
        drop(first);

        let mut resumed = monitor.subscribe();
        let retained = resumed.next().await.unwrap();
        let fresh = resumed.next().await.unwrap();

        assert_eq!(retained.printers, fresh.printers);
        assert!(fresh.updated_at > retained.updated_at);
        assert_eq!(collector.calls(), 2);
    }

    #[tokio::test(start_paused = true)]
    async fn resumed_subscription_delivers_retained_before_an_already_published_fresh_snapshot() {
        let collector = ScriptedCollector::new([success("retained"), success("fresh")]);
        let monitor = test_monitor(collector.clone());

        let mut first = monitor.subscribe();
        assert_eq!(first.next().await.unwrap().printers[0].name, "retained");
        drop(first);

        let mut resumed = monitor.subscribe();
        wait_for_calls(&collector, 2).await;

        assert_eq!(resumed.next().await.unwrap().printers[0].name, "retained");
        assert_eq!(resumed.next().await.unwrap().printers[0].name, "fresh");
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn an_aborted_generation_cannot_publish_after_a_new_generation_starts() {
        let collector =
            ScriptedCollector::new([success("retained"), success("stale"), success("fresh")]);
        let clock = BlockingSecondClock::starting_at(1_787_754_730);
        let monitor = test_monitor_with_clock(collector, clock.clone());

        let mut first = monitor.subscribe();
        assert_eq!(first.next().await.unwrap().printers[0].name, "retained");
        drop(first);

        let abandoned = monitor.subscribe();
        wait_for_blocked_clock(&clock).await;
        drop(abandoned);

        let mut resumed = monitor.subscribe();
        assert_eq!(resumed.next().await.unwrap().printers[0].name, "retained");
        clock.release();
        assert_eq!(resumed.next().await.unwrap().printers[0].name, "fresh");
        for _ in 0..10 {
            tokio::task::yield_now().await;
        }

        assert!(!resumed.receiver.has_changed().unwrap());
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn abort_and_resubscribe_never_overlap_collection_cycles() {
        let collector =
            BlockingSecondCollector::new([success("retained"), success("stale"), success("fresh")]);
        let monitor = test_monitor(collector.clone());

        let mut first = monitor.subscribe();
        assert_eq!(first.next().await.unwrap().printers[0].name, "retained");
        drop(first);

        let abandoned = monitor.subscribe();
        wait_for_blocked_collector(&collector).await;
        drop(abandoned);

        let mut resumed = monitor.subscribe();
        assert_eq!(resumed.next().await.unwrap().printers[0].name, "retained");
        let overlap = tokio::time::timeout(
            std::time::Duration::from_millis(100),
            collector.overlap.notified(),
        )
        .await
        .is_ok();
        collector.release();

        assert_eq!(resumed.next().await.unwrap().printers[0].name, "fresh");
        assert!(
            !overlap,
            "a resumed monitor must wait for the old collection"
        );
        assert_eq!(collector.maximum_active(), 1);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_queued_refresh_does_not_start_another_cycle_after_the_final_drop() {
        let collector = BlockingSecondCollector::new([
            success("retained"),
            success("stale"),
            success("unexpected"),
        ]);
        let monitor = test_monitor(collector.clone());

        let mut first = monitor.subscribe();
        assert_eq!(first.next().await.unwrap().printers[0].name, "retained");
        drop(first);

        let abandoned = monitor.subscribe();
        wait_for_blocked_collector(&collector).await;
        monitor.request_refresh();
        drop(abandoned);
        collector.release();

        let extra_cycle = tokio::time::timeout(
            std::time::Duration::from_millis(100),
            wait_for_collector_call(&collector, 3),
        )
        .await
        .is_ok();

        assert!(!extra_cycle);
        assert_eq!(collector.calls(), 2);
    }

    #[tokio::test(start_paused = true)]
    async fn an_invalidated_generation_waiting_for_the_collection_gate_does_not_collect() {
        let collector = ScriptedCollector::new([success("unexpected")]);
        let monitor = test_monitor(collector.clone());
        let gate = monitor.inner.collection.lock().await;
        {
            let mut state = monitor
                .inner
                .state
                .lock()
                .expect("the printer monitor state mutex should not be poisoned");
            state.subscribers = 1;
            state.generation = 1;
        }
        let waiting_monitor = monitor.clone();
        let waiting = tokio::spawn(async move {
            let _ = waiting_monitor.collect_and_publish(1, true).await;
        });

        tokio::task::yield_now().await;
        {
            let mut state = monitor
                .inner
                .state
                .lock()
                .expect("the printer monitor state mutex should not be poisoned");
            state.subscribers = 0;
            state.generation = 2;
        }
        drop(gate);
        tokio::time::timeout(std::time::Duration::from_secs(1), waiting)
            .await
            .expect("the invalidated waiting collection should finish")
            .expect("the waiting collection task should not panic");

        assert_eq!(collector.calls(), 0);
    }

    #[tokio::test(start_paused = true)]
    async fn final_drop_cancels_a_generation_waiting_for_the_collection_gate() {
        let collector = ScriptedCollector::new([success("unexpected")]);
        let monitor = test_monitor(collector.clone());
        let gate = monitor.inner.collection.lock().await;
        let subscription = monitor.subscribe();

        tokio::task::yield_now().await;
        drop(subscription);
        drop(gate);
        for _ in 0..10 {
            tokio::task::yield_now().await;
        }

        assert_eq!(collector.calls(), 0);
    }

    #[tokio::test(start_paused = true)]
    async fn a_stale_generation_is_fenced_from_publication() {
        let monitor = test_monitor(ScriptedCollector::new([success("stale")]));
        let mut receiver = monitor.inner.snapshots.subscribe();
        monitor.inner.snapshots.send_replace(Some(Snapshot {
            updated_at: OffsetDateTime::from_unix_timestamp(1_787_754_730)
                .expect("the fixed test timestamp should be valid"),
            warning: None,
            printers: vec![network_printer("retained", Availability::Connected)],
        }));
        receiver.borrow_and_update();
        {
            let mut state = monitor
                .inner
                .state
                .lock()
                .expect("the printer monitor state mutex should not be poisoned");
            state.subscribers = 1;
            state.generation = 2;
        }

        let _ = monitor.collect_and_publish(1, true).await;

        assert!(!receiver.has_changed().unwrap());
        assert_eq!(
            monitor.inner.snapshots.borrow().as_ref().unwrap().printers[0].name,
            "retained"
        );
    }

    #[tokio::test(start_paused = true)]
    async fn two_subscribers_share_one_collection_loop() {
        let collector = ScriptedCollector::new([success("kitchen")]);
        let monitor = test_monitor(collector.clone());

        let mut first = monitor.subscribe();
        let mut second = monitor.subscribe();

        assert_eq!(first.next().await.unwrap().printers[0].name, "kitchen");
        assert_eq!(second.next().await.unwrap().printers[0].name, "kitchen");
        assert_eq!(collector.calls(), 1);
    }

    #[tokio::test(start_paused = true)]
    async fn unchanged_ordinary_ticks_emit_nothing() {
        let collector = ScriptedCollector::new([success("kitchen"), success("kitchen")]);
        let monitor = test_monitor(collector.clone());
        let mut subscription = monitor.subscribe();

        subscription.next().await.unwrap();
        tokio::time::advance(std::time::Duration::from_secs(5)).await;
        wait_for_calls(&collector, 2).await;

        assert!(!subscription.receiver.has_changed().unwrap());
    }

    #[tokio::test(start_paused = true)]
    async fn changed_inventory_emits_a_snapshot() {
        let collector = ScriptedCollector::new([success("kitchen"), success("bar")]);
        let monitor = test_monitor(collector.clone());
        let mut subscription = monitor.subscribe();

        subscription.next().await.unwrap();
        tokio::time::advance(std::time::Duration::from_secs(5)).await;

        assert_eq!(subscription.next().await.unwrap().printers[0].name, "bar");
    }

    #[tokio::test(start_paused = true)]
    async fn dropping_the_final_subscription_aborts_a_blocked_collection() {
        let collector = BlockingCollector::new();
        let monitor = test_monitor(collector.clone());
        let subscription = monitor.subscribe();

        wait_for_calls(&collector, 1).await;
        drop(subscription);
        wait_for_cancellation(&collector).await;

        assert!(collector.was_cancelled());
    }

    #[tokio::test(start_paused = true)]
    async fn zero_subscribers_produce_no_collection_calls() {
        let collector = ScriptedCollector::new([]);
        let _monitor = test_monitor(collector.clone());

        tokio::task::yield_now().await;

        assert_eq!(collector.calls(), 0);
    }

    #[tokio::test(start_paused = true)]
    async fn a_failure_retains_printers_and_emits_a_warning() {
        let collector = ScriptedCollector::new([success("kitchen"), failure()]);
        let monitor = test_monitor(collector);
        let mut subscription = monitor.subscribe();

        let initial = subscription.next().await.unwrap();
        tokio::time::advance(std::time::Duration::from_secs(5)).await;
        let failed = subscription.next().await.unwrap();

        assert_eq!(failed.printers, initial.printers);
        assert_eq!(
            failed.warning.as_deref(),
            Some("printer name must not be blank")
        );
    }

    #[tokio::test(start_paused = true)]
    async fn repeated_identical_failure_is_silent() {
        let collector = ScriptedCollector::new([success("kitchen"), failure(), failure()]);
        let monitor = test_monitor(collector.clone());
        let mut subscription = monitor.subscribe();

        subscription.next().await.unwrap();
        tokio::time::advance(std::time::Duration::from_secs(5)).await;
        subscription.next().await.unwrap();
        tokio::time::advance(std::time::Duration::from_secs(5)).await;
        wait_for_calls(&collector, 3).await;

        assert!(!subscription.receiver.has_changed().unwrap());
    }

    #[tokio::test(start_paused = true)]
    async fn recovery_clears_a_warning() {
        let collector = ScriptedCollector::new([success("kitchen"), failure(), success("kitchen")]);
        let monitor = test_monitor(collector);
        let mut subscription = monitor.subscribe();

        subscription.next().await.unwrap();
        tokio::time::advance(std::time::Duration::from_secs(5)).await;
        assert!(subscription.next().await.unwrap().warning.is_some());
        tokio::time::advance(std::time::Duration::from_secs(5)).await;

        assert_eq!(subscription.next().await.unwrap().warning, None);
    }

    #[tokio::test(start_paused = true)]
    async fn request_refresh_wakes_an_active_monitor_but_does_nothing_while_idle() {
        let collector = ScriptedCollector::new([success("kitchen"), success("kitchen")]);
        let monitor = test_monitor(collector.clone());
        let mut subscription = monitor.subscribe();

        subscription.next().await.unwrap();
        monitor.request_refresh();
        wait_for_calls(&collector, 2).await;
        drop(subscription);

        monitor.request_refresh();
        tokio::task::yield_now().await;
        assert_eq!(collector.calls(), 2);
    }

    #[tokio::test(start_paused = true)]
    async fn a_refresh_queued_during_an_aborted_collection_does_not_leak_into_the_next_generation()
    {
        let collector = PausingSecondCollector::new([
            success("retained"),
            success("fresh"),
            success("unexpected"),
        ]);
        let monitor = test_monitor(collector.clone());

        let mut first = monitor.subscribe();
        assert_eq!(first.next().await.unwrap().printers[0].name, "retained");
        drop(first);

        let abandoned = monitor.subscribe();
        wait_for_calls(&collector, 2).await;
        monitor.request_refresh();
        drop(abandoned);

        let mut resumed = monitor.subscribe();
        assert_eq!(resumed.next().await.unwrap().printers[0].name, "retained");
        assert_eq!(resumed.next().await.unwrap().printers[0].name, "fresh");
        for _ in 0..10 {
            tokio::task::yield_now().await;
        }

        assert_eq!(collector.calls(), 3);
    }

    fn test_monitor(collector: impl Collector + 'static) -> PrinterMonitor {
        test_monitor_with_clock(collector, ScriptedClock::starting_at(1_787_754_730))
    }

    fn test_monitor_with_clock(
        collector: impl Collector + 'static,
        clock: impl Clock + 'static,
    ) -> PrinterMonitor {
        PrinterMonitor::with_dependencies(None, Arc::new(collector), Arc::new(clock))
    }

    fn success(name: &str) -> application::Result<list::Response> {
        Ok(list::Response {
            config_path: PathBuf::from("/tmp/printers.toml"),
            printers: vec![network_printer(name, Availability::Connected)],
        })
    }

    fn failure() -> application::Result<list::Response> {
        Err(application::ApplicationError::BlankPrinterName)
    }

    async fn wait_for_calls(collector: &impl CallCounter, expected: usize) {
        for _ in 0..10 {
            if collector.calls() >= expected {
                return;
            }
            tokio::task::yield_now().await;
        }
        panic!("the collector should have received {expected} calls");
    }

    async fn wait_for_cancellation(collector: &BlockingCollector) {
        for _ in 0..10 {
            if collector.was_cancelled() {
                return;
            }
            tokio::task::yield_now().await;
        }
        panic!("the blocked collection should have been cancelled");
    }

    async fn wait_for_blocked_clock(clock: &BlockingSecondClock) {
        loop {
            let blocked = clock.blocked.notified();
            if clock.is_blocked() {
                return;
            }
            blocked.await;
        }
    }

    async fn wait_for_blocked_collector(collector: &BlockingSecondCollector) {
        loop {
            let blocked = collector.blocked.notified();
            if collector.calls() >= 2 {
                return;
            }
            blocked.await;
        }
    }

    async fn wait_for_collector_call(collector: &BlockingSecondCollector, expected: usize) {
        loop {
            let started = collector.started.notified();
            if collector.calls() >= expected {
                return;
            }
            started.await;
        }
    }

    trait CallCounter {
        fn calls(&self) -> usize;
    }

    #[derive(Clone)]
    struct ScriptedCollector {
        responses: Arc<Mutex<VecDeque<application::Result<list::Response>>>>,
        calls: Arc<AtomicUsize>,
    }

    impl ScriptedCollector {
        fn new(responses: impl IntoIterator<Item = application::Result<list::Response>>) -> Self {
            Self {
                responses: Arc::new(Mutex::new(responses.into_iter().collect())),
                calls: Arc::new(AtomicUsize::new(0)),
            }
        }
    }

    impl CallCounter for ScriptedCollector {
        fn calls(&self) -> usize {
            self.calls.load(Ordering::SeqCst)
        }
    }

    impl Collector for ScriptedCollector {
        fn collect(&self, _request: list::Request) -> CollectionFuture {
            self.calls.fetch_add(1, Ordering::SeqCst);
            let response = self
                .responses
                .lock()
                .expect("the scripted responses mutex should not be poisoned")
                .pop_front()
                .expect("the test must script every collection");
            Box::pin(std::future::ready(response))
        }
    }

    #[derive(Clone)]
    struct BlockingCollector {
        calls: Arc<AtomicUsize>,
        cancelled: Arc<AtomicBool>,
    }

    impl BlockingCollector {
        fn new() -> Self {
            Self {
                calls: Arc::new(AtomicUsize::new(0)),
                cancelled: Arc::new(AtomicBool::new(false)),
            }
        }

        fn was_cancelled(&self) -> bool {
            self.cancelled.load(Ordering::SeqCst)
        }
    }

    impl CallCounter for BlockingCollector {
        fn calls(&self) -> usize {
            self.calls.load(Ordering::SeqCst)
        }
    }

    impl Collector for BlockingCollector {
        fn collect(&self, _request: list::Request) -> CollectionFuture {
            self.calls.fetch_add(1, Ordering::SeqCst);
            let cancelled = Arc::clone(&self.cancelled);
            Box::pin(async move {
                let _cancel = CancelOnDrop(cancelled);
                std::future::pending().await
            })
        }
    }

    #[derive(Clone)]
    struct PausingSecondCollector {
        responses: Arc<Mutex<VecDeque<application::Result<list::Response>>>>,
        calls: Arc<AtomicUsize>,
    }

    impl PausingSecondCollector {
        fn new(responses: impl IntoIterator<Item = application::Result<list::Response>>) -> Self {
            Self {
                responses: Arc::new(Mutex::new(responses.into_iter().collect())),
                calls: Arc::new(AtomicUsize::new(0)),
            }
        }
    }

    impl CallCounter for PausingSecondCollector {
        fn calls(&self) -> usize {
            self.calls.load(Ordering::SeqCst)
        }
    }

    impl Collector for PausingSecondCollector {
        fn collect(&self, _request: list::Request) -> CollectionFuture {
            let call = self.calls.fetch_add(1, Ordering::SeqCst) + 1;
            if call == 2 {
                return Box::pin(std::future::pending());
            }
            let response = self
                .responses
                .lock()
                .expect("the scripted responses mutex should not be poisoned")
                .pop_front()
                .expect("the test must script every collection");
            Box::pin(std::future::ready(response))
        }
    }

    #[derive(Clone)]
    struct BlockingSecondCollector {
        responses: Arc<Mutex<VecDeque<application::Result<list::Response>>>>,
        calls: Arc<AtomicUsize>,
        active: Arc<AtomicUsize>,
        maximum_active: Arc<AtomicUsize>,
        started: Arc<Notify>,
        blocked: Arc<Notify>,
        overlap: Arc<Notify>,
        release: Arc<(Mutex<bool>, Condvar)>,
    }

    impl BlockingSecondCollector {
        fn new(responses: impl IntoIterator<Item = application::Result<list::Response>>) -> Self {
            Self {
                responses: Arc::new(Mutex::new(responses.into_iter().collect())),
                calls: Arc::new(AtomicUsize::new(0)),
                active: Arc::new(AtomicUsize::new(0)),
                maximum_active: Arc::new(AtomicUsize::new(0)),
                started: Arc::new(Notify::new()),
                blocked: Arc::new(Notify::new()),
                overlap: Arc::new(Notify::new()),
                release: Arc::new((Mutex::new(false), Condvar::new())),
            }
        }

        fn maximum_active(&self) -> usize {
            self.maximum_active.load(Ordering::SeqCst)
        }

        fn release(&self) {
            let (released, wake) = &*self.release;
            *released
                .lock()
                .expect("the collector release mutex should not be poisoned") = true;
            wake.notify_all();
        }
    }

    impl CallCounter for BlockingSecondCollector {
        fn calls(&self) -> usize {
            self.calls.load(Ordering::SeqCst)
        }
    }

    impl Collector for BlockingSecondCollector {
        fn collect(&self, _request: list::Request) -> CollectionFuture {
            let call = self.calls.fetch_add(1, Ordering::SeqCst) + 1;
            self.started.notify_waiters();
            let active = self.active.fetch_add(1, Ordering::SeqCst) + 1;
            self.maximum_active.fetch_max(active, Ordering::SeqCst);
            if active > 1 {
                self.overlap.notify_waiters();
            }
            let activity = ActiveCollection(Arc::clone(&self.active));
            let response = self
                .responses
                .lock()
                .expect("the scripted responses mutex should not be poisoned")
                .pop_front()
                .expect("the test must script every collection");
            if call == 2 {
                self.blocked.notify_waiters();
                let (released, wake) = &*self.release;
                let mut released = released
                    .lock()
                    .expect("the collector release mutex should not be poisoned");
                while !*released {
                    released = wake
                        .wait(released)
                        .expect("the collector release mutex should not be poisoned");
                }
            }
            Box::pin(async move {
                let _active = activity;
                response
            })
        }
    }

    struct ActiveCollection(Arc<AtomicUsize>);

    impl Drop for ActiveCollection {
        fn drop(&mut self) {
            self.0.fetch_sub(1, Ordering::SeqCst);
        }
    }

    struct CancelOnDrop(Arc<AtomicBool>);

    impl Drop for CancelOnDrop {
        fn drop(&mut self) {
            self.0.store(true, Ordering::SeqCst);
        }
    }

    struct ScriptedClock {
        next_timestamp: Mutex<OffsetDateTime>,
    }

    impl ScriptedClock {
        fn starting_at(timestamp: i64) -> Self {
            Self {
                next_timestamp: Mutex::new(
                    OffsetDateTime::from_unix_timestamp(timestamp)
                        .expect("the fixed test timestamp should be valid"),
                ),
            }
        }
    }

    impl Clock for ScriptedClock {
        fn now(&self) -> OffsetDateTime {
            let mut timestamp = self
                .next_timestamp
                .lock()
                .expect("the scripted clock mutex should not be poisoned");
            let now = *timestamp;
            *timestamp += time::Duration::seconds(1);
            now
        }
    }

    #[derive(Clone)]
    struct BlockingSecondClock {
        calls: Arc<AtomicUsize>,
        blocked: Arc<Notify>,
        next_timestamp: Arc<Mutex<OffsetDateTime>>,
        release: Arc<(Mutex<bool>, Condvar)>,
    }

    impl BlockingSecondClock {
        fn starting_at(timestamp: i64) -> Self {
            Self {
                calls: Arc::new(AtomicUsize::new(0)),
                blocked: Arc::new(Notify::new()),
                next_timestamp: Arc::new(Mutex::new(
                    OffsetDateTime::from_unix_timestamp(timestamp)
                        .expect("the fixed test timestamp should be valid"),
                )),
                release: Arc::new((Mutex::new(false), Condvar::new())),
            }
        }

        fn is_blocked(&self) -> bool {
            self.calls.load(Ordering::SeqCst) >= 2
        }

        fn release(&self) {
            let (released, wake) = &*self.release;
            *released
                .lock()
                .expect("the clock release mutex should not be poisoned") = true;
            wake.notify_all();
        }
    }

    impl Clock for BlockingSecondClock {
        fn now(&self) -> OffsetDateTime {
            let call = self.calls.fetch_add(1, Ordering::SeqCst) + 1;
            let now = {
                let mut timestamp = self
                    .next_timestamp
                    .lock()
                    .expect("the scripted clock mutex should not be poisoned");
                let now = *timestamp;
                *timestamp += time::Duration::seconds(1);
                now
            };
            if call == 2 {
                self.blocked.notify_waiters();
                let (released, wake) = &*self.release;
                let mut released = released
                    .lock()
                    .expect("the clock release mutex should not be poisoned");
                while !*released {
                    released = wake
                        .wait(released)
                        .expect("the clock release mutex should not be poisoned");
                }
            }
            now
        }
    }

    fn network_printer(name: &str, availability: Availability) -> list::Printer {
        list::Printer {
            name: name.to_owned(),
            transport: Transport::Network,
            availability,
            profile: None,
            connection: ConnectionFacts::Network(NetworkConnectionFacts {
                host: "127.0.0.1".to_owned(),
                port: 9100,
            }),
        }
    }
}
