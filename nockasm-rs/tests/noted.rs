//! The strip contract of `nockasm::noted` over the shared corpus:
//! dressing and projecting is the identity, and lowering or rendering
//! annotated IR agrees with the bare pipeline — through the parser's
//! output and through the lift's (which exercises the `%nock` fallback
//! nodes the parser never produces).

mod common;

use nockasm::noted::{self, Noted};
use nockasm::{expand, lift, lower, parse, render};

#[test]
fn strip_is_the_identity_over_corpus() {
    for (name, src) in common::corpus() {
        let program = parse(&src).unwrap_or_else(|e| panic!("{name}: parse: {e}"));
        let noted = Noted::dress(&program.body, name.as_str());
        assert_eq!(
            noted.strip(),
            Ok(program.body.clone()),
            "{name}: strip(dress)"
        );
        let renamed = noted.map_note(|n| n.len());
        assert_eq!(
            renamed.strip(),
            Ok(program.body.clone()),
            "{name}: strip(map_note)"
        );

        let formula = expand(&src).unwrap_or_else(|e| panic!("{name}: expand: {e}"));
        let lifted = lift(&formula);
        assert_eq!(
            Noted::lift(&formula, ()).strip(),
            Ok(lifted),
            "{name}: strip(lift)"
        );
    }
}

#[test]
fn annotated_pipeline_agrees_with_bare_pipeline_over_corpus() {
    for (name, src) in common::corpus() {
        let program = parse(&src).unwrap_or_else(|e| panic!("{name}: parse: {e}"));
        let schema = program.schema.as_ref();
        let noted = Noted::dress(&program.body, Some((1u32, 1u32)));

        let want = expand(&src).unwrap_or_else(|e| panic!("{name}: expand: {e}"));
        let low = noted::lower(schema, &noted).unwrap_or_else(|e| panic!("{name}: lower: {e}"));
        assert_eq!(low, want, "{name}: noted::lower disagrees with expand");
        assert_eq!(
            low,
            lower(schema, &program.body).unwrap(),
            "{name}: noted::lower disagrees with lower"
        );
        assert_eq!(
            noted::render(schema, &noted).unwrap(),
            render(schema, &program.body),
            "{name}: noted::render disagrees with render"
        );

        let formula = want;
        let noted = Noted::lift(&formula, ());
        assert_eq!(
            noted::lower(None, &noted).unwrap(),
            formula,
            "{name}: lift soundness through the annotated IR"
        );
        assert_eq!(
            noted::render(None, &noted).unwrap(),
            render(None, &lift(&formula)),
            "{name}: noted::render disagrees with render on the lift"
        );
    }
}
