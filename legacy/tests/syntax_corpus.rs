use gdkit::syntax::parse;
use std::process::Command;

#[test]
#[ignore = "requires GODOT_SOURCE pointing to an engine repository with tag 4.7.2-stable"]
fn engine_syntax_corpus() {
    let repository = std::env::var_os("GODOT_SOURCE").expect("set GODOT_SOURCE");
    let revision = "4.7.2-stable";
    let git = |args: &[&str]| {
        let output = Command::new("git")
            .arg("-C")
            .arg(&repository)
            .args(args)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8(output.stdout).unwrap()
    };
    let listing = git(&[
        "ls-tree",
        "-r",
        "--name-only",
        revision,
        "modules/gdscript/tests/scripts",
    ]);
    let mut reconstructed = 0;
    let mut accepted = 0;
    let mut total = 0;
    let mut failures = Vec::new();
    for path in listing
        .lines()
        .filter(|path| path.ends_with(".gd") && !path.contains("/completion/"))
    {
        let source = git(&["show", &format!("{revision}:{path}")]);
        let parsed = parse(&source);
        let rebuilt: String = parsed
            .root()
            .tokens()
            .map(|token| &source[token.range.range()])
            .collect();
        assert_eq!(source, rebuilt, "lossless tree: {path}");
        reconstructed += 1;
        if path.contains("/errors/") || path.contains("/lsp/") {
            continue;
        }
        total += 1;
        if parsed.is_valid() {
            accepted += 1;
        } else {
            failures.push(format!(
                "{path}: {:?}",
                &parsed.errors()[..parsed.errors().len().min(3)]
            ));
        }
    }
    eprintln!(
        "Accepted {accepted}/{total} valid Godot {revision} scripts; reconstructed {reconstructed} scripts including error and LSP fixtures"
    );
    assert!(total > 300, "expected the full engine corpus");
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
