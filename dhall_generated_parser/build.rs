use std::env;
use std::fs::File;
use std::io::{BufRead, BufReader, Read, Write};
use std::path::Path;

use abnf_to_pest::render_rules_to_pest;

fn main() -> std::io::Result<()> {
    // TODO: upstream changes to grammar
    // let abnf_path = "../dhall-lang/standard/dhall.abnf";
    let abnf_path = "src/dhall.abnf";
    let visibility_path = "src/dhall.pest.visibility";
    let pest_path = "src/dhall.pest";
    println!("cargo:rerun-if-changed={}", abnf_path);
    println!("cargo:rerun-if-changed={}", visibility_path);

    let mut file = File::open(abnf_path)?;
    let mut data = Vec::new();
    file.read_to_end(&mut data)?;
    data.push('\n' as u8);

    let mut rules = abnf_to_pest::parse_abnf(&data)?;
    for line in BufReader::new(File::open(visibility_path)?).lines() {
        let line = line?;
        if line.len() >= 2 && &line[0..2] == "# " {
            rules.get_mut(&line[2..]).map(|x| x.silent = true);
        }
    }
    rules.remove("http");
    rules.remove("url_path");
    rules.remove("simple_label");
    rules.remove("nonreserved_label");
    rules.remove("expression");

    let mut file = File::create(pest_path)?;
    writeln!(&mut file, "// AUTO-GENERATED FILE. See build.rs.")?;
    writeln!(&mut file, "{}", render_rules_to_pest(rules).pretty(80))?;

    writeln!(&mut file)?;
    writeln!(
        &mut file,
        "simple_label = {{
              keyword ~ simple_label_next_char+
            | !keyword ~ simple_label_first_char ~ simple_label_next_char*
    }}"
    )?;
    // TODO: this is a cheat; actually implement inline headers instead
    writeln!(
        &mut file,
        "http = {{
            http_raw
            ~ (whsp
                ~ using
                ~ whsp1
                ~ (import_hashed | ^\"(\" ~ whsp ~ import_hashed ~ whsp ~ ^\")\"))?
    }}"
    )?;
    // TODO: hack; we'll need to upstream a change to the grammar
    writeln!(
        &mut file,
        r#"expression = {{
          lambda ~ whsp ~ ^"(" ~ whsp ~ nonreserved_label ~ whsp ~ ^":" ~ whsp1 ~ expression ~ whsp ~ ^")" ~ whsp ~ arrow ~ whsp ~ expression
          | if_ ~ whsp1 ~ expression ~ whsp ~ then ~ whsp1 ~ expression ~ whsp ~ else_ ~ whsp1 ~ expression
          | let_binding+ ~ in_ ~ whsp1 ~ expression
          | forall ~ whsp ~ ^"(" ~ whsp ~ nonreserved_label ~ whsp ~ ^":" ~ whsp1 ~ expression ~ whsp ~ ^")" ~ whsp ~ arrow ~ whsp ~ expression
          | operator_expression ~ whsp ~ arrow ~ whsp ~ expression
          | merge ~ whsp1 ~ import_expression ~ whsp1 ~ import_expression ~ whsp ~ ^":" ~ whsp1 ~ application_expression
          | empty_list_literal
          | toMap ~ whsp1 ~ import_expression ~ whsp ~ ^":" ~ whsp1 ~ application_expression
          | annotated_expression
    }}"#
    )?;
    writeln!(
        &mut file,
        r#"empty_list_literal = {{
          ^"[" ~ whsp ~ ^"]" ~ whsp ~ ^":" ~ whsp1 ~ application_expression
    }}"#
    )?;
    // TODO: this is a cheat; properly support RFC3986 URLs instead
    writeln!(&mut file, "url_path = _{{ path }}")?;
    writeln!(
        &mut file,
        "nonreserved_label = _{{
            !(builtin ~ !simple_label_next_char) ~ label
    }}"
    )?;
    writeln!(
        &mut file,
        "final_expression = ${{ SOI ~ complete_expression ~ EOI }}"
    )?;

    // Generate pest parser manually to avoid spurious recompilations
    let derived = {
        let pest_path = "dhall.pest";
        let pest = quote::quote! {
            #[grammar = #pest_path]
            pub struct DhallParser;
        };
        pest_generator::derive_parser(pest, false)
    };

    let out_dir = env::var("OUT_DIR").unwrap();
    let grammar_path = Path::new(&out_dir).join("grammar.rs");
    let mut file = File::create(grammar_path)?;
    writeln!(file, "pub struct DhallParser;\n{}", derived,)?;

    Ok(())
}
