//! Live checks against the Prism on this machine: backend selection by
//! priority, the `FREIGHT_FATE_SPEECH_BACKEND` override, the separate SAPI
//! event voice, and that configure passes values through untouched.
//!
//! One Prism context per process, on one thread at a time: every test takes
//! `LIVE_PRISM` and builds and drops its own context inside it. They skip
//! (pass, printing why) under `FREIGHT_FATE_NO_SPEECH` so CI without a screen reader is unaffected.

use std::sync::Mutex;

use freight_fate::speech::{
    pick_backend, pick_event_backend, PrismRegistry, Speech, SpeechSink, VoiceRegistry,
    EVENT_BACKEND,
};

static LIVE_PRISM: Mutex<()> = Mutex::new(());

fn live_prism_allowed() -> bool {
    if std::env::var_os("FREIGHT_FATE_NO_SPEECH").is_some_and(|v| !v.is_empty()) {
        eprintln!("skipping live Prism check: FREIGHT_FATE_NO_SPEECH is set");
        return false;
    }
    true
}

/// Registry names, priorities and live usability of every backend.
fn live_registry_table(ctx: &PrismRegistry) -> Vec<(String, i32, bool)> {
    (0..ctx.backend_count())
        .filter_map(|index| ctx.id_at(index))
        .map(|id| {
            let name = ctx.name_of(id).unwrap_or_default();
            let usable = ctx
                .acquire(id)
                .map(|backend| freight_fate::speech::usable(backend.as_ref()))
                .unwrap_or(false);
            (name, ctx.priority_of(id), usable)
        })
        .collect()
}

#[test]
fn live_pick_backend_prefers_the_highest_priority_usable_backend() {
    let _guard = LIVE_PRISM.lock().unwrap_or_else(|e| e.into_inner());
    if !live_prism_allowed() {
        return;
    }
    let Ok(ctx) = PrismRegistry::new() else {
        eprintln!("skipping live Prism check: context refused to start");
        return;
    };
    let table = live_registry_table(&ctx);
    eprintln!("prism registry: {table:?}");
    let narrator = freight_fate::speech::narrator_running();
    let expected = table
        .iter()
        .filter(|(name, _, usable)| *usable && (name != "UIA" || narrator))
        .map(|(name, priority, _)| {
            let priority = if name == "UIA" {
                freight_fate::speech::UIA_LAST_RESORT_PRIORITY
            } else {
                *priority
            };
            (priority, name.clone())
        })
        .max_by_key(|(priority, _)| *priority)
        .map(|(_, name)| name);
    let picked = pick_backend(&ctx, None).map(|backend| backend.name());
    assert_eq!(
        picked, expected,
        "pick_backend disagrees with the registry table"
    );
    if let Some(name) = &picked {
        eprintln!("picked main voice: {name}");
    } else {
        eprintln!("nothing on this machine can speak right now");
    }
}

#[test]
fn live_speech_backend_env_selects_sapi() {
    let _guard = LIVE_PRISM.lock().unwrap_or_else(|e| e.into_inner());
    if !live_prism_allowed() {
        return;
    }
    let Ok(ctx) = PrismRegistry::new() else {
        return;
    };
    let sapi_usable = live_registry_table(&ctx)
        .iter()
        .any(|(name, _, usable)| name == "SAPI" && *usable);
    if !sapi_usable {
        eprintln!("skipping: SAPI is not usable on this machine");
        return;
    }
    let picked = pick_backend(&ctx, Some("SAPI")).map(|backend| backend.name());
    assert_eq!(picked.as_deref(), Some("SAPI"));
    drop(ctx);

    // The same through the environment and `Speech::new()`, the way the
    // game reads it. Serialised by the mutex; removed again before leaving.
    std::env::set_var("FREIGHT_FATE_SPEECH_BACKEND", "SAPI");
    let mut speech = Speech::new(&freight_fate::speech::env());
    let name = speech.backend_name();
    let event = speech.event_backend_name();
    speech.shutdown();
    std::env::remove_var("FREIGHT_FATE_SPEECH_BACKEND");
    assert_eq!(name, "SAPI");
    // SAPI as the main voice leaves nothing to separate events onto, unless
    // another software voice (OneCore) stands in.
    assert_ne!(event, "SAPI");
    eprintln!("forced SAPI main; event voice: {event}");
}

#[test]
fn live_event_voice_is_sapi_and_separate_from_a_non_sapi_main() {
    let _guard = LIVE_PRISM.lock().unwrap_or_else(|e| e.into_inner());
    if !live_prism_allowed() {
        return;
    }
    let Ok(ctx) = PrismRegistry::new() else {
        return;
    };
    let table = live_registry_table(&ctx);
    let sapi_usable = table
        .iter()
        .any(|(name, _, usable)| name == "SAPI" && *usable);
    let Some(main) = pick_backend(&ctx, None) else {
        eprintln!("skipping: nothing can speak");
        return;
    };
    let event = pick_event_backend(&ctx, Some(main.as_ref()), EVENT_BACKEND);
    if main.name() == "SAPI" || !sapi_usable {
        assert!(event.is_none(), "nothing to separate onto");
        eprintln!(
            "main is {} and SAPI usable={sapi_usable}: no separate event voice",
            main.name()
        );
        return;
    }
    let event = event.expect("SAPI is usable and the main voice is not SAPI");
    assert_eq!(event.name(), "SAPI");
    assert_ne!(event.name(), main.name());
    let mut speech = Speech::with_registry(Box::new(ctx), None);
    assert!(speech.has_separate_event_voice());
    assert_eq!(speech.event_backend_name(), "SAPI");
    assert!(speech.event_supports_rate());
    assert!(speech
        .event_backend_options()
        .iter()
        .any(|name| name == "SAPI"));
    speech.shutdown();
}

#[test]
fn live_configure_clamps_nothing_and_sapi_lists_voices() {
    let _guard = LIVE_PRISM.lock().unwrap_or_else(|e| e.into_inner());
    if !live_prism_allowed() {
        return;
    }
    let Ok(context) = hearken::speech::prismer::Prism::new() else {
        return;
    };
    let Ok(sapi) = context.create(hearken::speech::prismer::BackendId::SAPI) else {
        eprintln!("skipping: no SAPI backend registered");
        return;
    };
    if sapi.initialize().is_err() {
        eprintln!("skipping: SAPI could not be initialized");
        return;
    }
    if !sapi
        .features()
        .contains(hearken::speech::prismer::Features::IS_SUPPORTED_AT_RUNTIME)
    {
        eprintln!("skipping: SAPI not usable at runtime");
        return;
    }
    // Values pass through the speech layer untouched: what goes in is what
    // the backend reports back. Checked at the Prism boundary, where a
    // getter exists.
    for (rate, pitch, volume) in [(0.8f32, 0.3f32, 0.5f32), (0.0, 1.0, 1.0)] {
        sapi.set_rate(rate).expect("set rate");
        sapi.set_pitch(pitch).expect("set pitch");
        sapi.set_volume(volume).expect("set volume");
        let got = (
            sapi.rate().unwrap(),
            sapi.pitch().unwrap(),
            sapi.volume().unwrap(),
        );
        eprintln!("SAPI set ({rate}, {pitch}, {volume}) -> {got:?}");
        assert!(
            (got.0 - rate).abs() < 0.05,
            "rate {rate} came back as {}",
            got.0
        );
        assert!(
            (got.1 - pitch).abs() < 0.05,
            "pitch {pitch} came back as {}",
            got.1
        );
        assert!(
            (got.2 - volume).abs() < 0.05,
            "volume {volume} came back as {}",
            got.2
        );
    }
    // Back to a sane middle so the next thing that speaks sounds normal.
    let _ = sapi.set_rate(0.5);
    let _ = sapi.set_pitch(0.5);
    let _ = sapi.set_volume(1.0);
    drop(sapi);
    drop(context);

    let Ok(registry) = PrismRegistry::new() else {
        return;
    };
    let mut speech = Speech::with_registry(Box::new(registry), Some("SAPI".to_string()));
    assert_eq!(speech.backend_name(), "SAPI");
    assert!(speech.supports_rate() && speech.supports_pitch() && speech.supports_volume());
    let voices = speech.voice_names();
    eprintln!("SAPI voices: {voices:?}");
    assert!(!voices.is_empty(), "SAPI lists no voices");
    // Through the speech layer itself: same values, no rescaling, no panic,
    // the voice stays bound.
    speech.configure(
        Some(0.8),
        Some(0.3),
        Some(0.5),
        voices.first().map(String::as_str),
    );
    assert_eq!(speech.config().rate, Some(0.8));
    assert_eq!(
        speech.config().voice.as_deref(),
        voices.first().map(String::as_str)
    );
    speech.configure(Some(0.5), Some(0.5), Some(1.0), None);
    assert!(speech.available());
    speech.shutdown();
}
