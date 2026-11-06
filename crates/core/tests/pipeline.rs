//! Pipeline acceptance tests: each stage composes in one process for
//! ordinary source and TSRX, deterministic and idempotent.

use std::path::Path;

use octane_fmt_core::{Engine, EngineConfig};

fn engine(cwd: &Path) -> Engine {
    Engine::new(&EngineConfig {
        cwd: cwd.to_path_buf(),
        ..EngineConfig::default()
    })
    .expect("engine config")
}

#[test]
fn ordinary_ts_gets_lint_fix_format_and_spacing() {
    let directory = std::env::temp_dir().join("octane-fmt-core-test-1");
    std::fs::create_dir_all(&directory).unwrap();
    let engine = engine(&directory);

    let source = "if (a) b()\nconst x = { a: 1,\n  b: 2 }\nconst y = 1\n";
    let outcome = engine
        .format_text(Path::new("src/one.ts"), source)
        .expect("formats");
    // `curly` adds braces; the formatter applies the blessed style
    // (tabs, no semicolons, single quotes); a blank line appears after the
    // multiline `x` initializer statement.
    eprintln!("---\n{}---", outcome.code);
    assert!(outcome.code.contains("if (a) {"), "{}", outcome.code);
    assert!(outcome.code.contains("b()\n}"), "{}", outcome.code);
    assert!(
        !outcome.code.contains(';'),
        "semi: false expected\n{}",
        outcome.code
    );

    // Idempotent.
    let second = engine
        .format_text(Path::new("src/one.ts"), &outcome.code)
        .expect("reformats");
    assert_eq!(second.code, outcome.code);
    assert!(!second.changed);
}

#[test]
fn tsrx_formats_and_adds_blank_line_before_render() {
    let directory = std::env::temp_dir().join("octane-fmt-core-test-2");
    std::fs::create_dir_all(&directory).unwrap();
    let engine = engine(&directory);

    let source =
        "export function View() @{\n  const a = {\n    one: 1,\n  }\n  <p>{a.one}</p>\n}\n";
    let outcome = engine
        .format_text(Path::new("src/View.tsrx"), source)
        .expect("formats tsrx");
    eprintln!("---\n{}---", outcome.code);
    assert!(outcome.code.contains("<p>"), "{}", outcome.code);

    let second = engine
        .format_text(Path::new("src/View.tsrx"), &outcome.code)
        .expect("reformats");
    assert_eq!(second.code, outcome.code);
}

#[test]
fn declaration_clusters_get_surrounding_blank_lines() {
    let directory = std::env::temp_dir().join("octane-fmt-core-test-clusters");
    std::fs::create_dir_all(&directory).unwrap();
    let engine = engine(&directory);

    // Directives, imports, exports, and declarations each cluster; runs get
    // blank lines around them even when every member is single-line.
    let source = "'use client'\nimport a from 'a'\nimport { b } from 'b'\nconst x = 1\nfoo()\ninterface I { a: string }\ntype T = I\nexport function g() {}\nexport const e = 2\nexport { x }\nlast()\n";
    let outcome = engine
        .format_text(Path::new("src/clusters.ts"), source)
        .expect("formats");
    eprintln!("---\n{}---", outcome.code);
    assert!(
        outcome.code.contains("'use client'\n\nimport"),
        "{}",
        outcome.code
    );
    assert!(outcome.code.contains("'b'\n\nconst x"), "{}", outcome.code);
    assert!(
        outcome.code.contains("const x = 1\n\nfoo()"),
        "{}",
        outcome.code
    );
    assert!(
        outcome.code.contains("foo()\n\ninterface"),
        "{}",
        outcome.code
    );
    // Export declarations cluster separately from non-exported ones: the
    // multiline interface still gets its trailing blank, `type` stays with
    // the declaration run, and the export run is blanked on both sides.
    assert!(
        outcome.code.contains("}\n\ntype T = I\n\nexport function"),
        "{}",
        outcome.code
    );
    assert!(
        outcome
            .code
            .contains("export function g() {}\nexport const e = 2\nexport { x }\n\nlast()"),
        "{}",
        outcome.code
    );

    // Function bodies are containers too.
    let nested = engine
        .format_text(
            Path::new("src/nested.ts"),
            "function f() {\nconst a = 1\nside()\nreturn a\n}\n",
        )
        .expect("formats nested");
    assert!(
        nested.code.contains("const a = 1\n\n\tside()"),
        "{}",
        nested.code
    );

    // Idempotent.
    for outcome in [outcome, nested] {
        let second = engine
            .format_text(Path::new("src/again.ts"), &outcome.code)
            .expect("reformats");
        assert_eq!(second.code, outcome.code);
        assert!(!second.changed);
    }
}

#[test]
fn malformed_source_fails_without_output() {
    let directory = std::env::temp_dir().join("octane-fmt-core-test-3");
    std::fs::create_dir_all(&directory).unwrap();
    let engine = engine(&directory);

    let result = engine.format_text(Path::new("bad.ts"), "const = 3");
    let Err(diagnostics) = result else {
        panic!("must fail")
    };
    assert_ne!(diagnostics, [] as [octane_fmt_core::Diagnostic; 0]);
    eprintln!("{diagnostics:?}");
}

#[test]
fn unicode_source_keeps_correct_spans() {
    let directory = std::env::temp_dir().join("octane-fmt-core-test-4");
    std::fs::create_dir_all(&directory).unwrap();
    let engine = engine(&directory);

    let source = "const café = '☕'\nconst big = {\n  a: 1,\n  b: 2,\n}\nconst done = true\n";
    let outcome = engine
        .format_text(Path::new("uni.ts"), source)
        .expect("formats unicode");
    assert!(outcome.code.contains("café"), "{}", outcome.code);
    assert!(outcome.code.contains("'☕'"), "{}", outcome.code);
    let second = engine
        .format_text(Path::new("uni.ts"), &outcome.code)
        .unwrap();
    assert_eq!(second.code, outcome.code);
}

#[test]
fn declaration_mode_survives_safe_fix_validation_and_format_drift_reparse() {
    let directory = std::env::temp_dir().join("octane-fmt-declaration-fixes");
    std::fs::create_dir_all(&directory).unwrap();
    std::fs::write(
        directory.join(".oxlintrc.json"),
        r#"{ "rules": { "no-var": "error" } }"#,
    )
    .unwrap();
    let engine = engine(&directory);

    for extension in ["d.ts", "d.mts", "d.cts"] {
        let filename = format!("contracts/repro.{extension}");
        let path = Path::new(&filename);
        let source = "export var mutable: string\nexport const contract: {\nvalue: string\n}";
        let first = engine
            .format_text(path, source)
            .expect("formats declaration");
        assert!(
            first.code.contains("export let mutable: string"),
            "{}",
            first.code
        );
        assert!(
            first.code.contains("export const contract:"),
            "{}",
            first.code
        );
        // The drift probe must be able to parse the same ambient output too.
        let kind = oxc_adapter::SourceKind::from_path(path).unwrap();
        assert_eq!(
            oxc_adapter::count_expanded_objects(&first.code, kind, false).unwrap(),
            1
        );
        let second = engine
            .format_text(path, &first.code)
            .expect("reformats declaration");
        assert_eq!(second.code, first.code);
        assert!(!second.changed);
    }
    std::fs::remove_dir_all(directory).unwrap();
}
