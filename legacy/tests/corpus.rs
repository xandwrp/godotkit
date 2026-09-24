use std::process::Command;

use gdkit::{
    formatter::{Options, format_source},
    syntax::parse,
};

#[test]
#[ignore = "requires GODOT_SOURCE pointing to an engine repository with tag 4.7.2-stable"]
fn engine_corpus_formats_stably_and_reparses() {
    let repository =
        std::env::var_os("GODOT_SOURCE").expect("set GODOT_SOURCE to the engine repository");
    let listing = Command::new("git")
        .arg("-C")
        .arg(&repository)
        .args([
            "ls-tree",
            "-r",
            "--name-only",
            "4.7.2-stable",
            "modules/gdscript/tests/scripts",
        ])
        .output()
        .unwrap();
    assert!(listing.status.success());
    let mut count = 0;
    for path in String::from_utf8(listing.stdout).unwrap().lines() {
        if !path.ends_with(".gd")
            || path.contains("/errors/")
            || path.contains("/completion/")
            || path.contains("/lsp/")
        {
            continue;
        }
        let original = Command::new("git")
            .arg("-C")
            .arg(&repository)
            .args(["show", &format!("4.7.2-stable:{path}")])
            .output()
            .unwrap();
        assert!(original.status.success(), "{path}");
        let source = String::from_utf8(original.stdout).unwrap();
        let formatted = format_source(&source, &Options::default())
            .unwrap_or_else(|error| panic!("{path}: {error}"));
        assert!(parse(&formatted).is_valid(), "{path}");
        let again = format_source(&formatted, &Options::default()).unwrap();
        assert_eq!(formatted, again, "{path}");
        count += 1;
    }
    assert!(count > 300);
    println!("Verified {count} Godot 4.7.2 scripts");
}
