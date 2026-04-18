//! Integration tests for `apply_keymap_overrides`.
//!
//! Exercises the full loop: parse TOML → build profile → layer user
//! bindings on top → verify lookups + warnings.

use arx_config::{
    apply_keymap_overrides, BindingEntry, Config, UnbindEntry, Warning,
};
use arx_keymap::{parse_sequence, profiles::emacs, Lookup};

fn always_exists(_: &str) -> bool {
    true
}

fn only_known(name: &str) -> bool {
    matches!(name, "buffer.save" | "command-palette.open" | "editor.quit")
}

#[test]
fn binding_applies_on_top_of_profile() {
    let mut profile = emacs();
    let bindings = vec![BindingEntry {
        keys: "C-c p".into(),
        command: "command-palette.open".into(),
    }];
    let warnings = apply_keymap_overrides(&mut profile, &bindings, &[], &always_exists);
    assert!(warnings.is_empty());

    let chords = parse_sequence("C-c p").unwrap();
    match profile.global.lookup(&chords) {
        Lookup::Command(cmd) => assert_eq!(&*cmd.name, "command-palette.open"),
        other => panic!("expected Command, got {other:?}"),
    }
}

#[test]
fn unbind_shadows_profile_binding() {
    let mut profile = emacs();
    // Emacs profile binds C-x C-s to buffer.save.
    let seq = parse_sequence("C-x C-s").unwrap();
    assert!(matches!(profile.global.lookup(&seq), Lookup::Command(_)));

    let unbind = vec![UnbindEntry {
        keys: "C-x C-s".into(),
    }];
    let warnings = apply_keymap_overrides(&mut profile, &[], &unbind, &always_exists);
    assert!(warnings.is_empty());

    assert!(matches!(profile.global.lookup(&seq), Lookup::Unbound));
}

#[test]
fn unknown_command_produces_warning() {
    let mut profile = emacs();
    let bindings = vec![BindingEntry {
        keys: "C-c q".into(),
        command: "definitely.not.a.command".into(),
    }];
    let warnings = apply_keymap_overrides(&mut profile, &bindings, &[], &only_known);
    assert_eq!(warnings.len(), 1);
    assert!(matches!(
        warnings[0],
        Warning::UnknownCommand { .. }
    ));

    // Binding did NOT apply.
    let seq = parse_sequence("C-c q").unwrap();
    assert!(matches!(profile.global.lookup(&seq), Lookup::NoMatch));
}

#[test]
fn invalid_key_sequence_produces_warning() {
    let mut profile = emacs();
    let bindings = vec![BindingEntry {
        keys: String::new(),
        command: "buffer.save".into(),
    }];
    let warnings = apply_keymap_overrides(&mut profile, &bindings, &[], &always_exists);
    assert_eq!(warnings.len(), 1);
    assert!(matches!(
        warnings[0],
        Warning::InvalidKeySequence { .. }
    ));
}

#[test]
fn bindings_apply_in_file_order_last_wins() {
    let mut profile = emacs();
    let bindings = vec![
        BindingEntry {
            keys: "C-c p".into(),
            command: "buffer.save".into(),
        },
        BindingEntry {
            keys: "C-c p".into(),
            command: "editor.quit".into(),
        },
    ];
    let warnings = apply_keymap_overrides(&mut profile, &bindings, &[], &only_known);
    assert!(warnings.is_empty());

    let seq = parse_sequence("C-c p").unwrap();
    match profile.global.lookup(&seq) {
        Lookup::Command(cmd) => assert_eq!(&*cmd.name, "editor.quit"),
        other => panic!("expected Command, got {other:?}"),
    }
}

#[test]
fn full_toml_parse_then_apply() {
    let src = r#"
[keymap]
profile = "emacs"

[[keymap.bindings]]
keys = "C-c p"
command = "command-palette.open"

[[keymap.unbind]]
keys = "C-z"
"#;
    let cfg: Config = toml::from_str(src).unwrap();
    let mut profile = emacs();
    let warnings = apply_keymap_overrides(
        &mut profile,
        &cfg.keymap.bindings,
        &cfg.keymap.unbind,
        &always_exists,
    );
    assert!(warnings.is_empty());

    let seq = parse_sequence("C-c p").unwrap();
    match profile.global.lookup(&seq) {
        Lookup::Command(cmd) => assert_eq!(&*cmd.name, "command-palette.open"),
        other => panic!("expected Command, got {other:?}"),
    }
    let seq_z = parse_sequence("C-z").unwrap();
    assert!(matches!(profile.global.lookup(&seq_z), Lookup::Unbound));
}
