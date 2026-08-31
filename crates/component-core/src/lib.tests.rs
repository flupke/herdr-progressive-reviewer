use super::*;

#[derive(Debug, Eq, PartialEq)]
enum TestAction {
    Reload,
}

struct CountChanged(u8);
struct NameChanged(u8);

#[derive(Debug)]
struct TestComponent {
    count: u8,
}

impl Component<TestAction> for TestComponent {
    fn register_subscriptions(subscriptions: &mut ComponentSubscriptions<'_, Self, TestAction>) {
        subscriptions.subscribe(Self::count_changed);
        subscriptions.subscribe(Self::name_changed);
    }
}

impl TestComponent {
    fn new(_events: EventPublisher) -> Self {
        Self { count: 0 }
    }

    fn count_changed(&mut self, event: &CountChanged) -> Vec<TestAction> {
        self.count += event.0;
        vec![TestAction::Reload]
    }

    fn name_changed(&mut self, event: &NameChanged) {
        self.count = self.count.saturating_add(event.0);
    }
}

#[test]
fn typed_event_publication_returns_actions() {
    let mut event_bus = ComponentEventBus::new();
    event_bus.mount(TestComponent::new);

    let results = event_bus
        .publish(CountChanged(2))
        .expect("typed publication must succeed");

    assert_eq!(results.len(), 1);
    assert_eq!(
        results.into_iter().next().unwrap().into_actions(),
        [TestAction::Reload]
    );
}

#[test]
fn removing_an_exact_component_removes_only_its_subscriptions() {
    let mut event_bus = ComponentEventBus::new();
    let removed = event_bus.mount(TestComponent::new);
    let retained = event_bus.mount(TestComponent::new);

    assert!(event_bus.remove(removed));
    let results = event_bus
        .publish(CountChanged(2))
        .expect("publication after removal must succeed");

    assert_eq!(results.len(), 1);
    assert_eq!(event_bus.get::<TestComponent>(retained).unwrap().count, 2);
}

#[test]
fn component_lookup_uses_the_exact_target() {
    let mut event_bus = ComponentEventBus::new();
    let first = event_bus.mount(|_events| TestComponent { count: 3 });
    let second = event_bus.mount(|_events| TestComponent { count: 7 });

    assert_eq!(event_bus.get::<TestComponent>(first).unwrap().count, 3);
    assert_eq!(event_bus.get::<TestComponent>(second).unwrap().count, 7);
}

#[test]
fn publication_rejects_a_mismatched_mounted_component() {
    let mut event_bus = ComponentEventBus::new();
    let target = event_bus.mount(TestComponent::new);
    let _original_component =
        event_bus.replace_component_for_test::<TestComponent, _>(target, String::new());

    let result = event_bus.publish(CountChanged(1));

    assert_eq!(result, Err(DispatchError::ComponentTypeMismatch));
}

#[test]
fn publication_rejects_a_mismatched_event_value() {
    let mut event_bus = ComponentEventBus::new();
    event_bus.mount(TestComponent::new);
    let event =
        EventEnvelope::with_declared_type_for_test::<NameChanged, CountChanged>(NameChanged(1));

    let result = event_bus.publish_envelope(event);

    assert_eq!(result, Err(DispatchError::EventTypeMismatch));
}

#[derive(Clone)]
struct InputEvent;

struct GlobalComponent;

impl GlobalComponent {
    #[allow(clippy::unused_self)]
    fn input(&mut self, _event: InputEvent) -> Vec<&'static str> {
        vec!["global"]
    }
}

impl Component<&'static str> for GlobalComponent {
    fn register_subscriptions(subscriptions: &mut ComponentSubscriptions<'_, Self, &'static str>) {
        subscriptions.subscribe_input(InputScope::Global, AnyInput, Self::input);
    }
}

struct FocusedComponent;

impl Component<&'static str> for FocusedComponent {
    fn register_subscriptions(subscriptions: &mut ComponentSubscriptions<'_, Self, &'static str>) {
        subscriptions.subscribe_input(InputScope::Focused, AnyInput, Self::input);
    }
}

impl FocusedComponent {
    #[allow(clippy::unused_self)]
    fn input(&mut self, _event: InputEvent) -> Vec<&'static str> {
        vec!["focused"]
    }
}

struct NoMatch;

impl<C> InputMatcher<C, InputEvent> for NoMatch {
    type Output = InputEvent;

    fn resolve(&mut self, _component: &C, _input: &InputEvent) -> InputResolution<Self::Output> {
        InputResolution::NoMatch
    }
}

struct AwaitingMoreInput;

impl<C> InputMatcher<C, InputEvent> for AwaitingMoreInput {
    type Output = InputEvent;

    fn resolve(&mut self, _component: &C, _input: &InputEvent) -> InputResolution<Self::Output> {
        InputResolution::AwaitingMoreInput
    }
}

struct SelectiveFocusedComponent;

impl Component<&'static str> for SelectiveFocusedComponent {
    fn register_subscriptions(subscriptions: &mut ComponentSubscriptions<'_, Self, &'static str>) {
        subscriptions.subscribe_input(InputScope::Focused, NoMatch, Self::input);
    }
}

impl SelectiveFocusedComponent {
    #[allow(clippy::unused_self)]
    fn input(&mut self, _event: InputEvent) -> Vec<&'static str> {
        unreachable!("the handler must run only after its matcher matches")
    }
}

struct PrefixGlobalComponent;

impl Component<&'static str> for PrefixGlobalComponent {
    fn register_subscriptions(subscriptions: &mut ComponentSubscriptions<'_, Self, &'static str>) {
        subscriptions.subscribe_input(InputScope::Global, AwaitingMoreInput, Self::input);
    }
}

impl PrefixGlobalComponent {
    #[allow(clippy::unused_self)]
    fn input(&mut self, _event: InputEvent) -> Vec<&'static str> {
        unreachable!("the handler must run only after its matcher matches")
    }
}

#[test]
fn focused_input_is_delivered_to_the_selected_component() {
    let mut event_bus = ComponentEventBus::new();
    let focused = event_bus.mount(|_| FocusedComponent);
    let event = EventEnvelope::new(InputEvent);

    let results = event_bus
        .dispatch_input(&event, focused)
        .unwrap()
        .into_results();
    let actions = results
        .into_iter()
        .flat_map(DispatchResult::into_actions)
        .collect::<Vec<_>>();

    assert_eq!(actions, ["focused"]);
}

#[test]
fn global_input_is_delivered_to_each_matching_subscription() {
    let mut event_bus = ComponentEventBus::new();
    event_bus.mount(|_| GlobalComponent);
    event_bus.mount(|_| GlobalComponent);
    let event = EventEnvelope::new(InputEvent);

    let dispatch = event_bus.dispatch_global_input(&event).unwrap();
    assert!(!dispatch.global_input_pending());
    let results = dispatch.into_results();
    let actions = results
        .into_iter()
        .flat_map(DispatchResult::into_actions)
        .collect::<Vec<_>>();

    assert_eq!(actions, ["global", "global"]);
}

#[test]
fn matched_focused_input_stops_before_global_handlers() {
    let mut event_bus = ComponentEventBus::new();
    event_bus.mount(|_| GlobalComponent);
    let focused = event_bus.mount(|_| FocusedComponent);

    let dispatch = event_bus
        .dispatch_input(&EventEnvelope::new(InputEvent), focused)
        .unwrap();
    let actions = dispatch
        .into_results()
        .into_iter()
        .flat_map(DispatchResult::into_actions)
        .collect::<Vec<_>>();

    assert_eq!(actions, ["focused"]);
}

#[test]
fn unmatched_focused_subscription_falls_back_to_global_subscriptions() {
    let mut event_bus = ComponentEventBus::new();
    event_bus.mount(|_| GlobalComponent);
    let focused = event_bus.mount(|_| SelectiveFocusedComponent);

    let dispatch = event_bus
        .dispatch_input(&EventEnvelope::new(InputEvent), focused)
        .unwrap();
    let actions = dispatch
        .into_results()
        .into_iter()
        .flat_map(DispatchResult::into_actions)
        .collect::<Vec<_>>();

    assert_eq!(actions, ["global"]);
}

#[test]
fn incomplete_global_subscription_waits_without_running_its_handler() {
    let mut event_bus = ComponentEventBus::new();
    event_bus.mount(|_| PrefixGlobalComponent);

    let dispatch = event_bus
        .dispatch_global_input(&EventEnvelope::new(InputEvent))
        .unwrap();

    assert!(dispatch.global_input_pending());
    assert!(dispatch.into_results().is_empty());
}

#[test]
fn ordinary_publication_does_not_deliver_to_input_subscriptions() {
    let mut event_bus = ComponentEventBus::new();
    event_bus.mount(|_| FocusedComponent);

    let results = event_bus.publish(InputEvent).unwrap();

    assert!(results.is_empty());
}

struct PublishRequest;
struct PublishedChange;

struct PublishingComponent(EventPublisher);

impl PublishingComponent {
    fn publish(&mut self, _event: &PublishRequest) {
        self.0.publish(PublishedChange);
    }
}

impl Component<&'static str> for PublishingComponent {
    fn register_subscriptions(subscriptions: &mut ComponentSubscriptions<'_, Self, &'static str>) {
        subscriptions.subscribe(Self::publish);
    }
}

struct ObservingComponent;

impl ObservingComponent {
    #[allow(clippy::unused_self)]
    fn observe(&mut self, _event: &PublishedChange) -> Vec<&'static str> {
        vec!["observed"]
    }
}

impl Component<&'static str> for ObservingComponent {
    fn register_subscriptions(subscriptions: &mut ComponentSubscriptions<'_, Self, &'static str>) {
        subscriptions.subscribe(Self::observe);
    }
}

#[test]
fn component_publication_reaches_other_components_in_the_same_publication() {
    let mut event_bus = ComponentEventBus::new();
    event_bus.mount(PublishingComponent);
    event_bus.mount(|_| ObservingComponent);

    let actions = event_bus
        .publish(PublishRequest)
        .expect("publication must succeed")
        .into_iter()
        .flat_map(DispatchResult::into_actions)
        .collect::<Vec<_>>();

    assert_eq!(actions, ["observed"]);
}

struct RepeatingEvent;

struct RepeatingComponent(EventPublisher);

impl RepeatingComponent {
    fn repeat(&mut self, _event: &RepeatingEvent) {
        self.0.publish(RepeatingEvent);
    }
}

impl Component<()> for RepeatingComponent {
    fn register_subscriptions(subscriptions: &mut ComponentSubscriptions<'_, Self, ()>) {
        subscriptions.subscribe(Self::repeat);
    }
}

#[test]
fn event_cycles_stop_at_the_delivery_limit() {
    let mut event_bus = ComponentEventBus::new();
    event_bus.mount(RepeatingComponent);

    let result = event_bus.publish(RepeatingEvent);

    assert_eq!(result, Err(DispatchError::EventCycleLimitExceeded));
}
