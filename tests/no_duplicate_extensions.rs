use std::collections::HashSet;

use bat::assets::HighlightingAssets;

#[test]
fn no_duplicate_extensions() {
    const KNOWN_EXCEPTIONS: &[&str] = &[
        // The '.h' extension currently appears in multiple syntaxes: C, C++, Objective C,
        // Objective C++
        "h",
        // In addition to the standard JavaScript syntax in 'Packages', we also ship the
        // 'Javascript (Babel)' syntax.
        "js",
        // The "Ruby Haml" syntax also comes with a '.sass' extension. However, we make sure
        // that 'sass' is mapped to the 'Sass' syntax.
        "sass",
        // The '.fs' extension appears in F# and GLSL.
        // We default to F#.
        "fs",
        // SystemVerilog and Verilog both use .v files.
        // We default to Verilog.
        "v",
    ];

    let assets = HighlightingAssets::with_no_cache();

    let mut extensions = HashSet::new();

    for syntax in assets.get_syntaxes() {
        for extension in &syntax.file_extensions {
            assert!(
                KNOWN_EXCEPTIONS.contains(&extension.as_str()) || extensions.insert(extension),
                "File extension / pattern \"{}\" appears twice in the syntax set",
                extension
            );
        }
    }
}
