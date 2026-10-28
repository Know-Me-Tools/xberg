use super::*;
use std::cell::Cell;

#[test]
fn should_retain_successful_secondary_handles_across_calls_gh2306() {
    let dictionaries = SecondaryDictionaries::new("models-a", "ita+eng+deu", 1);
    let loads = Cell::new(0);
    for _ in 0..3 {
        let call = dictionaries.for_call();
        let loaded = call.initialize(0, |path, language, oem| {
            assert_eq!((path, language, oem), ("models-a", "eng", 1));
            loads.set(loads.get() + 1);
            Some(loads.get())
        });
        assert_eq!(loaded, Some(&1));
        assert_eq!(
            call.initialize(0, |_, _, _| panic!("reuse the loaded engine")),
            Some(&1)
        );
    }
    assert_eq!(loads.get(), 1);
    assert!(
        dictionaries.entries[1].api.get().is_none(),
        "unused German must remain unloaded"
    );
}

#[test]
fn should_deduplicate_languages_without_loading_unused_dictionaries_gh2306() {
    let dictionaries = SecondaryDictionaries::new("models", " ita + eng + ita + eng + deu ", 3);
    assert_eq!(dictionaries.entries.len(), 2);
    assert!(dictionaries.entries.iter().all(|entry| entry.api.get().is_none()));
    let call = dictionaries.for_call();
    assert_eq!(
        call.initialize(1, |path, language, oem| {
            assert_eq!((path, language, oem), ("models", "deu", 3));
            Some("German")
        }),
        Some(&"German")
    );
    assert!(
        dictionaries.entries[0].api.get().is_none(),
        "unused English must remain unloaded"
    );
}

#[test]
fn should_memoize_failed_initialization_per_call_and_retry_later_gh2306() {
    let dictionaries = SecondaryDictionaries::new("models", "ita+eng", 1);
    let attempts = Cell::new(0);
    let first = dictionaries.for_call();
    assert_eq!(
        first.initialize(0, |_, _, _| {
            attempts.set(attempts.get() + 1);
            None::<usize>
        }),
        None
    );
    assert_eq!(
        first.initialize(0, |_, _, _| panic!("do not retry per rejected word")),
        None
    );
    let next = dictionaries.for_call();
    assert_eq!(
        next.initialize(0, |_, _, _| {
            attempts.set(attempts.get() + 1);
            Some(7)
        }),
        Some(&7)
    );
    assert_eq!(
        dictionaries
            .for_call()
            .initialize(0, |_, _, _| panic!("retain successful retry")),
        Some(&7)
    );
    assert_eq!(attempts.get(), 2);
}

#[test]
fn should_not_open_single_language_or_over_limit_secondary_engines_gh2306() {
    let single = SecondaryDictionaries::<usize>::new("models", "eng+eng", 1);
    assert!(single.entries.is_empty());
    assert_eq!(
        single
            .for_call()
            .initialize(0, |_, _, _| panic!("single-language lookup")),
        None
    );
    let many = SecondaryDictionaries::<usize>::new("models", "eng+ita+deu+fra+spa+rus+por", 1);
    assert!(many.too_many_languages);
    assert_eq!(many.entries.len(), MAX_SECONDARY_DICTIONARIES + 1);
    assert_eq!(
        many.for_call()
            .initialize(0, |_, _, _| panic!("too many secondary engines")),
        None
    );
    assert!(many.entries.iter().all(|entry| entry.api.get().is_none()));
}
