// Ports test/platform/settings.test.cpp.

use feature::{Value, ValueObject};
use mln_actor::platform_settings::{
    EXPERIMENTAL_THREAD_PRIORITY_DATABASE, EXPERIMENTAL_THREAD_PRIORITY_FILE, EXPERIMENTAL_THREAD_PRIORITY_NETWORK,
    EXPERIMENTAL_THREAD_PRIORITY_WORKER, Settings,
};

fn object(entries: &[(&str, Value)]) -> ValueObject {
    entries.iter().map(|(k, v)| ((*k).to_owned(), v.clone())).collect()
}

// gtest: Settings.SetAndGet
#[test]
fn set_and_get() {
    let settings = Settings::get_instance();

    let low_prio_value = Value::Double(19.0);
    settings.set(EXPERIMENTAL_THREAD_PRIORITY_WORKER, low_prio_value.clone());
    let thread_priority = settings.get(EXPERIMENTAL_THREAD_PRIORITY_WORKER);
    assert_eq!(low_prio_value, thread_priority);
    assert_eq!(low_prio_value, settings.get("maplibre_thread_priority_worker"));

    let network_prio_value = Value::Double(-20.0);
    let database_prio_value = Value::Double(-10.0);

    let multiple_values = object(&[
        (EXPERIMENTAL_THREAD_PRIORITY_DATABASE, database_prio_value.clone()),
        (EXPERIMENTAL_THREAD_PRIORITY_NETWORK, network_prio_value.clone()),
    ]);
    settings.set_values(&multiple_values);

    let keys = [
        EXPERIMENTAL_THREAD_PRIORITY_WORKER,
        EXPERIMENTAL_THREAD_PRIORITY_DATABASE,
        EXPERIMENTAL_THREAD_PRIORITY_NETWORK,
        EXPERIMENTAL_THREAD_PRIORITY_FILE,
    ];

    let result = settings.get_values(&keys);
    let mut values = object(&[
        (EXPERIMENTAL_THREAD_PRIORITY_WORKER, low_prio_value),
        (EXPERIMENTAL_THREAD_PRIORITY_DATABASE, database_prio_value),
        (EXPERIMENTAL_THREAD_PRIORITY_NETWORK, network_prio_value),
        (EXPERIMENTAL_THREAD_PRIORITY_FILE, Value::Null),
    ]);
    assert_eq!(values, result);

    values = object(&[
        (EXPERIMENTAL_THREAD_PRIORITY_WORKER, Value::Null),
        (EXPERIMENTAL_THREAD_PRIORITY_DATABASE, Value::Null),
        (EXPERIMENTAL_THREAD_PRIORITY_NETWORK, Value::Null),
        (EXPERIMENTAL_THREAD_PRIORITY_FILE, Value::Null),
    ]);
    settings.set_values(&values);
    assert_eq!(values, settings.get_values(&keys));
}
